use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use schemars::JsonSchema;
use serde::Deserialize;

use super::git::{changed_files_between, inspect_repository, is_ancestor, resolve_commit_oid};
use super::{
    AgentDelivery, AgentWorktreeDelivery, DeliveryScope, TaskCoordinator, TaskRunRecord,
    ThreadExecutionStatus, WorkCompletionKind, WorkCompletionRecord, WorkUnitStatus,
};
use crate::agent::worktree::git_compatible_path;
use crate::tool::{
    FunctionToolDefinition, NamespaceDescriptor, RegisteredTool, SubagentContext, ToolEntry,
    ToolExecutionResult, ToolSourceId, ToolSourceMetadata,
};
use crate::turn::ToolEffect;
use crate::{AgentProgressStage, AgentRuntimeHandle, AgentSnapshot, TurnEngine};

#[derive(Debug, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum CompletionResultKindInput {
    Delivery,
    NoDelivery,
}

#[derive(Debug, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CompletionResultInput {
    /// Selects delivery or noDelivery validation.
    kind: CompletionResultKindInput,
    /// Full Git commit id or an unambiguous abbreviation of at least 7 hex characters.
    /// Required for delivery and forbidden for noDelivery.
    head_commit: Option<String>,
    /// Commands run and their outcomes, or evidence that no delivery was required.
    verification_summary: String,
}

#[derive(Debug, PartialEq, Eq)]
enum ValidatedCompletionResultInput {
    Delivery {
        head_commit: String,
        verification_summary: String,
    },
    NoDelivery {
        verification_summary: String,
    },
}

impl TryFrom<CompletionResultInput> for ValidatedCompletionResultInput {
    type Error = anyhow::Error;

    fn try_from(input: CompletionResultInput) -> Result<Self> {
        match (input.kind, input.head_commit) {
            (CompletionResultKindInput::Delivery, Some(head_commit)) => Ok(Self::Delivery {
                head_commit,
                verification_summary: input.verification_summary,
            }),
            (CompletionResultKindInput::Delivery, None) => {
                bail!("delivery requires headCommit")
            }
            (CompletionResultKindInput::NoDelivery, Some(_)) => {
                bail!("noDelivery must not include headCommit")
            }
            (CompletionResultKindInput::NoDelivery, None) => Ok(Self::NoDelivery {
                verification_summary: input.verification_summary,
            }),
        }
    }
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
        thread_id: &str,
        runtime: AgentRuntimeHandle,
        snapshot: &AgentSnapshot,
        active_task_run: Option<&TaskRunRecord>,
    ) {
        if active_task_run.is_none() {
            return;
        }
        // Task 协调工具统一发布到 task 来源（task 命名空间，延迟加载）。
        let source = ToolSourceId::task();
        let metadata = || {
            ToolSourceMetadata::new(source.clone()).with_namespace(NamespaceDescriptor::new(
                "task",
                "Task coordination, review, delivery, and completion tools.",
            ))
        };
        let mut entries = Vec::new();
        if snapshot.identity.parent_id.is_none() {
            // planner 复用框架统一的 send_message（parent→direct-child）调度子代理；
            // 不再注册 Task 专用 send_message。
            entries.push(ToolEntry::new(
                self.task_spawn_executor_tool(thread_id, runtime.clone()),
                metadata(),
            ));
            entries.push(ToolEntry::new(
                self.task_update_design_tool(thread_id),
                metadata(),
            ));
            entries.push(ToolEntry::new(
                self.task_record_merge_tool(thread_id, runtime.clone()),
                metadata(),
            ));
            entries.push(ToolEntry::new(
                self.task_request_delivery_review_tool(thread_id, runtime.clone()),
                metadata(),
            ));
            entries.push(ToolEntry::new(
                self.task_request_integrated_review_tool(thread_id, runtime.clone()),
                metadata(),
            ));
            entries.push(ToolEntry::new(
                self.task_status_tool(thread_id, Some(runtime.clone())),
                metadata(),
            ));
            entries.push(ToolEntry::new(
                self.read_review_round_tool(thread_id),
                metadata(),
            ));
            entries.push(ToolEntry::new(
                self.read_review_file_coverage_tool(thread_id),
                metadata(),
            ));
            entries.push(ToolEntry::new(
                self.task_complete_tool(thread_id),
                metadata(),
            ));
            entries.push(ToolEntry::new(
                self.task_stop_tool(thread_id, runtime.clone()),
                metadata(),
            ));
        } else {
            match snapshot.identity.role.as_str() {
                "executor" => {
                    entries.push(ToolEntry::new(
                        self.report_completion_tool(runtime),
                        metadata(),
                    ));
                }
                "reviewer" => {
                    entries.push(ToolEntry::new(
                        self.read_review_file_coverage_tool(thread_id),
                        metadata(),
                    ));
                    entries.push(ToolEntry::new(
                        self.review_exit_tool(thread_id, Some(runtime)),
                        metadata(),
                    ));
                }
                "explorer" | "planner" => {}
                _ => {}
            }
        }
        let _ = core.register_source_tools(source, entries);
    }

    pub(crate) fn report_completion_tool(
        self: &Arc<Self>,
        runtime: AgentRuntimeHandle,
    ) -> RegisteredTool {
        let coordinator = self.clone();
        FunctionToolDefinition::<CompletionResultInput>::new(
            "report_completion",
            "Report a clean executor result with top-level kind, headCommit, and verificationSummary fields, then end the current turn for mandatory delivery review.",
        )
        .registered(move |result: CompletionResultInput, context| {
                let coordinator = coordinator.clone();
                let runtime = runtime.clone();
                async move {
                    let subagent = context
                        .active_subagent
                        .as_ref()
                        .context("report_completion requires an active executor")?;
                    let completion = coordinator
                        .report_completion(subagent, context.workspace.root(), result)
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
                            /* detail */ None,
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
            })
        .with_effect(ToolEffect::BranchControl)
    }

    async fn report_completion(
        &self,
        subagent: &SubagentContext,
        caller_workspace: &Path,
        result: CompletionResultInput,
    ) -> Result<WorkCompletionRecord> {
        let repository = inspect_repository(caller_workspace, true).await?;
        let canonical_caller = git_compatible_path(
            std::fs::canonicalize(caller_workspace)
                .context("failed to resolve caller workspace path")?,
        );
        let scope = self
            .store
            .resolve_active_completion_scope(
                &subagent.id,
                &canonical_caller.to_string_lossy(),
                &repository.branch,
            )
            .await?
            .context("active completion scope not found for this executor worktree")?;
        ensure_completion_scope_is_open(&scope)?;
        match ValidatedCompletionResultInput::try_from(result)? {
            ValidatedCompletionResultInput::Delivery {
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
                        &scope.work_unit.id,
                        WorkCompletionKind::Delivery,
                        Some(&delivery),
                        delivery.verification_summary.as_str(),
                    )
                    .await
            }
            ValidatedCompletionResultInput::NoDelivery {
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
    if subagent.role != "executor" {
        bail!("report_completion may only be called by the assigned executor");
    }
    if !is_direct_task_child(subagent, &scope.run.root_thread_id)
        || scope.work_unit.task_run_id != scope.run.id
        || scope.work_unit.executor_thread_id.as_deref() != Some(subagent.id.as_str())
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

fn is_direct_task_child(subagent: &SubagentContext, root_thread_id: &str) -> bool {
    let root_agent_id = crate::studio::agent_host::root_agent_id(root_thread_id);
    subagent.depth == 1 && subagent.parent_id.as_deref() == Some(root_agent_id.as_str())
}

fn ensure_completion_scope_is_open(scope: &DeliveryScope) -> Result<()> {
    if scope.work_unit.execution_status != ThreadExecutionStatus::Running
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

fn normalized_path(path: &Path) -> String {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let path = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        path.to_lowercase()
    } else {
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::deserialize_tool_input;

    #[test]
    fn completion_input_schema_is_a_flat_object() {
        let schema =
            FunctionToolDefinition::<CompletionResultInput>::new("report_completion", "test")
                .input_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema.get("oneOf").is_none());
        let properties = schema["properties"].as_object().expect("input properties");
        assert!(properties.contains_key("kind"));
        assert!(properties.contains_key("headCommit"));
        assert!(properties.contains_key("verificationSummary"));
        assert!(!properties.contains_key("result"));
        let required = schema["required"].as_array().expect("required fields");
        assert!(required.iter().any(|field| field == "kind"));
        assert!(required.iter().any(|field| field == "verificationSummary"));
        assert!(!required.iter().any(|field| field == "headCommit"));
    }

    #[test]
    fn completion_input_deserializes_top_level_scalar_fields() {
        let delivery = deserialize_tool_input::<CompletionResultInput>(
            "report_completion",
            serde_json::json!({
                "kind": "delivery",
                "headCommit": "0123456789abcdef",
                "verificationSummary": "tests passed"
            }),
        )
        .expect("flat delivery input");
        assert_eq!(
            delivery,
            CompletionResultInput {
                kind: CompletionResultKindInput::Delivery,
                head_commit: Some("0123456789abcdef".to_string()),
                verification_summary: "tests passed".to_string(),
            }
        );

        let no_delivery = deserialize_tool_input::<CompletionResultInput>(
            "report_completion",
            serde_json::json!({
                "kind": "noDelivery",
                "verificationSummary": "no repository change required"
            }),
        )
        .expect("flat no-delivery input");
        assert_eq!(
            no_delivery,
            CompletionResultInput {
                kind: CompletionResultKindInput::NoDelivery,
                head_commit: None,
                verification_summary: "no repository change required".to_string(),
            }
        );
    }

    #[test]
    fn completion_input_rejects_invalid_head_commit_combinations() {
        let missing_delivery_head =
            ValidatedCompletionResultInput::try_from(CompletionResultInput {
                kind: CompletionResultKindInput::Delivery,
                head_commit: None,
                verification_summary: "tests passed".to_string(),
            })
            .expect_err("delivery without headCommit must fail");
        assert_eq!(
            missing_delivery_head.to_string(),
            "delivery requires headCommit"
        );

        let unexpected_no_delivery_head =
            ValidatedCompletionResultInput::try_from(CompletionResultInput {
                kind: CompletionResultKindInput::NoDelivery,
                head_commit: Some("0123456789abcdef".to_string()),
                verification_summary: "no repository change required".to_string(),
            })
            .expect_err("noDelivery with headCommit must fail");
        assert_eq!(
            unexpected_no_delivery_head.to_string(),
            "noDelivery must not include headCommit"
        );
    }
}
