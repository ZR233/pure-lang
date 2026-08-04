use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use super::git::{changed_files_between, inspect_repository, is_ancestor, resolve_commit_oid};
use super::owned_path::OwnedPath;
use super::{
    AgentDelivery, AgentOutcomeStatus, AgentWorktreeDelivery, DeliveryScope,
    DeliveryScopeResolution, TaskCoordinator, WorkCompletionKind, WorkCompletionRecord,
    WorkUnitStatus,
};
use crate::tool::{
    RegisteredTool, SubagentContext, ToolExecutionResult, ToolInputSchemaField,
    strict_tool_input_schema,
};
use crate::turn::ToolEffect;
use crate::{AgentProgressStage, AgentRuntimeHandle, AgentSnapshot, TurnEngine};

#[derive(Debug, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind",
    deny_unknown_fields
)]
enum CompletionResultInput {
    Delivery {
        head_commit: String,
        verification_summary: String,
    },
    NoDelivery {
        verification_summary: String,
    },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReportCompletionInput {
    result: CompletionResultInput,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TaskSendMessageInput {
    target: String,
    message: String,
}

#[derive(Clone, Copy)]
struct CompletionValidation<'a> {
    scope: &'a DeliveryScope,
    subagent: &'a SubagentContext,
    caller_workspace: &'a Path,
    verification_summary: &'a str,
}

impl TaskCoordinator {
    pub(crate) fn install_tools(
        self: &Arc<Self>,
        core: &mut TurnEngine,
        session_id: &str,
        runtime: AgentRuntimeHandle,
        snapshot: &AgentSnapshot,
    ) {
        if snapshot.identity.parent_id.is_none() {
            core.register_tool(self.task_send_message_tool(session_id, runtime.clone()));
            core.register_tool(self.task_spawn_executor_tool(session_id, runtime.clone()));
            core.register_tool(self.task_update_design_tool(session_id));
            core.register_tool(self.task_merge_agent_tool(session_id, runtime.clone()));
            core.register_tool(self.task_request_delivery_review_tool(session_id, runtime.clone()));
            core.register_tool(
                self.task_request_integrated_review_tool(session_id, runtime.clone()),
            );
            core.register_tool(self.task_status_tool(session_id));
            core.register_tool(self.task_complete_tool(session_id));
            core.register_tool(self.task_stop_tool(session_id, runtime.clone()));
            self.register_conflict_tools(core, session_id, runtime);
            return;
        }
        match snapshot.identity.role.as_str() {
            "executor" => {
                core.register_tool(self.report_completion_tool(runtime));
            }
            "reviewer" => {
                core.register_tool(self.review_exit_tool(session_id, None));
            }
            "explorer" | "planner" => {}
            _ => {}
        }
    }

    fn task_send_message_tool(
        self: &Arc<Self>,
        session_id: &str,
        runtime: AgentRuntimeHandle,
    ) -> RegisteredTool {
        let coordinator = self.clone();
        let session_id = session_id.to_string();
        RegisteredTool::from_typed_fallible_execution_result(
            "send_message",
            "Send a message to a direct Task child when its durable work state accepts input.",
            strict_tool_input_schema([
                ToolInputSchemaField::required("target", serde_json::json!({"type":"string"})),
                ToolInputSchemaField::required("message", serde_json::json!({"type":"string"})),
            ]),
            move |input: TaskSendMessageInput, _context| {
                let coordinator = coordinator.clone();
                let session_id = session_id.clone();
                let runtime = runtime.clone();
                async move {
                    let target = pl_core::AgentId::new(input.target)?;
                    let snapshot = runtime
                        .snapshot(target.clone())
                        .await
                        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
                    let root = crate::studio::agent_host::root_agent_id(&session_id);
                    if snapshot.identity.parent_id.as_ref() != Some(&root) {
                        bail!("send_message target is not a direct child of this Task planner");
                    }
                    if snapshot.identity.role.as_str() == "executor" {
                        coordinator
                            .store
                            .authorize_executor_message(&session_id, target.as_str())
                            .await?;
                    }
                    let turn_id = runtime
                        .submit_current_session(
                            target.clone(),
                            pl_core::AgentCurrentSessionSubmitRequest::start(input.message)
                                .with_presentation(pl_core::MailboxPresentation::Hidden),
                        )
                        .await
                        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
                    ToolExecutionResult::<serde_json::Value>::json(serde_json::json!({
                        "target": target,
                        "turnId": turn_id,
                    }))
                    .map_err(anyhow::Error::from)
                }
            },
        )
        .with_effect(ToolEffect::AgentControl)
    }

    pub(crate) fn report_completion_tool(
        self: &Arc<Self>,
        runtime: AgentRuntimeHandle,
    ) -> RegisteredTool {
        let coordinator = self.clone();
        RegisteredTool::from_typed_fallible_execution_result(
            "report_completion",
            "Report a clean executor result for mandatory delivery review and end the current turn.",
            strict_tool_input_schema([ToolInputSchemaField::required(
                "result",
                serde_json::json!({
                    "oneOf": [
                        {
                            "type": "object",
                            "additionalProperties": false,
                            "required": ["kind", "headCommit", "verificationSummary"],
                            "properties": {
                                "kind": { "const": "delivery" },
                                "headCommit": {
                                    "type": "string",
                                    "description": "Full Git commit id or an unambiguous hexadecimal abbreviation of at least 7 characters."
                                },
                                "verificationSummary": { "type": "string" }
                            }
                        },
                        {
                            "type": "object",
                            "additionalProperties": false,
                            "required": ["kind", "verificationSummary"],
                            "properties": {
                                "kind": { "const": "noDelivery" },
                                "verificationSummary": { "type": "string" }
                            }
                        }
                    ]
                }),
            )]),
            move |input: ReportCompletionInput, context| {
                let coordinator = coordinator.clone();
                let runtime = runtime.clone();
                async move {
                    let subagent = context
                        .active_subagent
                        .as_ref()
                        .context("report_completion requires an active executor")?;
                    let completion = coordinator
                        .report_completion(subagent, &context.workspace_root, input.result)
                        .await?;
                    if let Err(error) = runtime
                        .report_progress(
                            pl_core::AgentId::new(subagent.id.clone())?,
                            AgentProgressStage::ReadyForReview,
                            format!(
                                "completion revision {} is ready for delivery review",
                                completion.revision
                            ),
                            "wait for the planner to request an independent reviewer".to_string(),
                        )
                        .await
                    {
                        tracing::warn!(
                            agent_id = %subagent.id,
                            completion_id = %completion.id,
                            completion_revision = completion.revision,
                            error_bytes = error.to_string().len(),
                            "completion was committed but its directory progress projection failed"
                        );
                    }
                    let mut output =
                        ToolExecutionResult::<serde_json::Value>::json(completion)
                            .map_err(anyhow::Error::from)?;
                    output.ends_turn = true;
                    Ok::<_, anyhow::Error>(output)
                }
            },
        )
        .with_effect(ToolEffect::BranchControl)
    }

    async fn report_completion(
        &self,
        subagent: &SubagentContext,
        caller_workspace: &Path,
        result: CompletionResultInput,
    ) -> Result<WorkCompletionRecord> {
        let repository = inspect_repository(caller_workspace, true).await?;
        let canonical_caller = std::fs::canonicalize(caller_workspace)
            .context("failed to resolve caller workspace path")?;
        let resolution = self
            .store
            .resolve_active_completion_scope(
                &subagent.id,
                &canonical_caller.to_string_lossy(),
                &repository.branch,
            )
            .await?
            .context("active completion scope not found for this executor worktree")?;
        let scope = match resolution {
            DeliveryScopeResolution::Resolved(scope) => *scope,
            DeliveryScopeResolution::MissingWorkUnit(_) => {
                bail!("executor outcome has no work unit")
            }
        };
        ensure_completion_scope_is_open(&scope)?;
        match result {
            CompletionResultInput::Delivery {
                head_commit,
                verification_summary,
            } => {
                let delivery = self
                    .validate_delivery(
                        CompletionValidation {
                            scope: &scope,
                            subagent,
                            caller_workspace,
                            verification_summary: &verification_summary,
                        },
                        &head_commit,
                    )
                    .await?;
                self.store
                    .create_work_completion(
                        &scope.outcome.id,
                        &scope.work_unit.id,
                        WorkCompletionKind::Delivery,
                        Some(&delivery),
                        delivery.verification_summary.as_str(),
                    )
                    .await
            }
            CompletionResultInput::NoDelivery {
                verification_summary,
            } => {
                let verification_summary = validate_common(
                    CompletionValidation {
                        scope: &scope,
                        subagent,
                        caller_workspace,
                        verification_summary: &verification_summary,
                    },
                    &repository,
                )?;
                if repository.head != scope.work_unit.base_commit {
                    bail!("noDelivery requires worktree HEAD to equal its base commit");
                }
                self.store
                    .create_work_completion(
                        &scope.outcome.id,
                        &scope.work_unit.id,
                        WorkCompletionKind::NoDelivery,
                        None,
                        verification_summary,
                    )
                    .await
            }
        }
    }

    async fn validate_delivery(
        &self,
        validation: CompletionValidation<'_>,
        supplied_head: &str,
    ) -> Result<AgentDelivery> {
        let snapshot = inspect_repository(validation.caller_workspace, true).await?;
        let verification_summary = validate_common(validation, &snapshot)?;
        let supplied_head = supplied_head.trim();
        if supplied_head.len() < 7 || !supplied_head.chars().all(|ch| ch.is_ascii_hexdigit()) {
            bail!("headCommit must be a full commit id or at least 7 hexadecimal characters");
        }
        let resolved_supplied_head =
            resolve_commit_oid(&snapshot.workspace_root, supplied_head).await?;
        if snapshot.head != resolved_supplied_head {
            bail!("headCommit does not match worktree HEAD");
        }
        let base_commit = validation.scope.work_unit.base_commit.as_str();
        if snapshot.head == base_commit {
            bail!("delivery HEAD must advance beyond its base commit");
        }
        if !is_ancestor(&snapshot.workspace_root, base_commit, &snapshot.head).await? {
            bail!("delivery HEAD must descend from its base commit");
        }
        let changed_files =
            changed_files_between(&snapshot.workspace_root, base_commit, &snapshot.head).await?;
        validate_owned_paths(&validation.scope.work_unit.owned_paths, &changed_files)?;
        Ok(AgentDelivery {
            worktree: AgentWorktreeDelivery {
                path: snapshot.workspace_root.to_string_lossy().to_string(),
                branch: snapshot.branch,
            },
            base_commit: base_commit.to_string(),
            head_commit: snapshot.head,
            changed_files,
            verification_summary: verification_summary.to_string(),
        })
    }
}

fn validate_common<'a>(
    validation: CompletionValidation<'a>,
    repository: &super::git::RepositorySnapshot,
) -> Result<&'a str> {
    let CompletionValidation {
        scope,
        subagent,
        caller_workspace,
        verification_summary,
    } = validation;
    if subagent.role != "executor" || scope.outcome.role != "executor" {
        bail!("report_completion may only be called by the assigned executor");
    }
    if !is_direct_task_child(subagent, &scope.run.session_id)
        || scope.outcome.owner_path != "/root"
        || scope.outcome.initiated_by != "planner"
        || scope.outcome.task_run_id != scope.run.id
        || scope.work_unit.task_run_id != scope.run.id
        || scope.work_unit.agent_id.as_deref() != Some(subagent.id.as_str())
    {
        bail!("executor does not own this work unit");
    }
    let verification_summary = verification_summary.trim();
    if verification_summary.is_empty() {
        bail!("verificationSummary must not be empty");
    }
    if normalized_path(&repository.git_common_dir)
        != normalized_path(Path::new(&scope.run.git_common_dir))
        || normalized_path(caller_workspace)
            != normalized_path(Path::new(&scope.work_unit.worktree_path))
        || repository.branch != scope.work_unit.branch
    {
        bail!("caller repository does not match the assigned executor worktree");
    }
    Ok(verification_summary)
}

fn is_direct_task_child(subagent: &SubagentContext, root_session_id: &str) -> bool {
    let root_agent_id = crate::studio::agent_host::root_agent_id(root_session_id);
    subagent.depth == 1 && subagent.parent_id.as_deref() == Some(root_agent_id.as_str())
}

fn ensure_completion_scope_is_open(scope: &DeliveryScope) -> Result<()> {
    if scope.outcome.status != AgentOutcomeStatus::Running
        || !matches!(
            scope.work_unit.status,
            WorkUnitStatus::Running
                | WorkUnitStatus::AwaitingCompletion
                | WorkUnitStatus::ChangesRequested
        )
    {
        bail!("work unit is not accepting a completion");
    }
    Ok(())
}

fn validate_owned_paths(owned_paths: &[String], changed_files: &[String]) -> Result<()> {
    let owned_paths = owned_paths
        .iter()
        .map(|path| OwnedPath::parse(path))
        .collect::<Result<Vec<_>>>()?;
    for changed_file in changed_files {
        if !owned_paths.iter().any(|owned| owned.matches(changed_file)) {
            bail!("changed file `{changed_file}` is outside ownedPaths");
        }
    }
    Ok(())
}

fn normalized_path(path: &Path) -> String {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let path = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        path.to_lowercase()
    } else {
        path
    }
}
