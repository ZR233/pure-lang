use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;

use super::git::{changed_files_between, inspect_repository, is_ancestor};
use super::owned_path::OwnedPath;
use super::{
    AgentDelivery, AgentOutcomeStatus, AgentWorktreeDelivery, DeliveryScope,
    DeliveryScopeResolution, TaskCoordinator, WorkUnitStatus,
};
use crate::tool::{
    RegisteredTool, SubagentContext, ToolExecutionResult, ToolInputSchemaField,
    strict_tool_input_schema,
};
use crate::turn::ToolEffect;
use crate::{AgentRuntimeHandle, AgentSnapshot, TurnEngine};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SubmitDeliveryInput {
    head_commit: String,
    verification_summary: String,
}

struct DeliveryValidation<'a> {
    scope: &'a DeliveryScope,
    subagent: &'a SubagentContext,
    caller_workspace: &'a Path,
    supplied_head: &'a str,
    verification_summary: &'a str,
}

struct CommittedDelivery {
    task_run_id: String,
    outcome_id: String,
    agent_id: String,
    delivery: AgentDelivery,
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
            core.register_tool(self.task_spawn_executor_tool(session_id, runtime.clone()));
            core.register_tool(self.task_update_design_tool(session_id));
            core.register_tool(self.task_merge_agent_tool(session_id, runtime.clone()));
            core.register_tool(self.task_request_review_tool(session_id, runtime.clone()));
            core.register_tool(self.task_complete_tool(session_id));
            core.register_tool(self.task_stop_tool(session_id, runtime.clone()));
            self.register_conflict_tools(core, session_id, runtime);
            return;
        }
        match snapshot.identity.role.as_str() {
            "executor" => {
                core.register_tool(
                    self.submit_delivery_tool(session_id.to_string(), Some(runtime)),
                );
            }
            "reviewer" => {
                core.register_tool(self.review_exit_tool(session_id, Some(runtime)));
            }
            "explorer" | "planner" => {}
            _ => {}
        }
    }

    #[cfg(test)]
    pub(crate) async fn submit_delivery(
        &self,
        subagent: &SubagentContext,
        caller_workspace: impl AsRef<Path>,
        supplied_head: &str,
        verification_summary: &str,
    ) -> Result<AgentDelivery> {
        Ok(self
            .commit_delivery(
                subagent,
                caller_workspace,
                supplied_head,
                verification_summary,
            )
            .await?
            .delivery)
    }

    async fn commit_delivery(
        &self,
        subagent: &SubagentContext,
        caller_workspace: impl AsRef<Path>,
        supplied_head: &str,
        verification_summary: &str,
    ) -> Result<CommittedDelivery> {
        let caller_workspace = caller_workspace.as_ref();
        let repository = inspect_repository(caller_workspace, false).await?;
        let canonical_caller = std::fs::canonicalize(caller_workspace)
            .context("failed to resolve caller workspace path")?;
        let resolution = self
            .store
            .resolve_active_delivery_scope(
                &subagent.id,
                &canonical_caller.to_string_lossy(),
                &repository.branch,
            )
            .await?
            .context("active delivery scope not found for this executor worktree")?;
        let scope = match resolution {
            DeliveryScopeResolution::Resolved(scope) => *scope,
            DeliveryScopeResolution::MissingWorkUnit(outcome) => {
                let error = anyhow!("executor outcome has no work unit");
                self.store
                    .mark_agent_delivery_waiting(&outcome.id, None, &error.to_string())
                    .await?;
                return Err(error);
            }
        };
        ensure_delivery_scope_is_open(&scope)?;

        let result = self
            .validate_delivery(DeliveryValidation {
                scope: &scope,
                subagent,
                caller_workspace,
                supplied_head,
                verification_summary,
            })
            .await;
        let delivery = match result {
            Ok(delivery) => delivery,
            Err(error) => {
                self.store
                    .mark_agent_delivery_waiting(
                        &scope.outcome.id,
                        Some(&scope.work_unit.id),
                        &error.to_string(),
                    )
                    .await?;
                return Err(error);
            }
        };

        self.store
            .complete_agent_delivery(&scope.outcome.id, &scope.work_unit.id, delivery.clone())
            .await?;
        Ok(CommittedDelivery {
            task_run_id: scope.run.id,
            outcome_id: scope.outcome.id,
            agent_id: scope.outcome.agent_id,
            delivery,
        })
    }

    pub(crate) fn submit_delivery_tool(
        self: &Arc<Self>,
        session_id: String,
        runtime: Option<AgentRuntimeHandle>,
    ) -> RegisteredTool {
        let coordinator = self.clone();
        RegisteredTool::from_typed_fallible_execution_result(
            "submit_delivery",
            "Validate and persist a committed executor delivery.",
            strict_tool_input_schema([
                ToolInputSchemaField::required(
                    "headCommit",
                    serde_json::json!({ "type": "string" }),
                ),
                ToolInputSchemaField::required(
                    "verificationSummary",
                    serde_json::json!({ "type": "string" }),
                ),
            ]),
            move |arguments: SubmitDeliveryInput, context| {
                let coordinator = coordinator.clone();
                let runtime = runtime.clone();
                let session_id = session_id.clone();
                async move {
                    let subagent = context
                        .active_subagent
                        .as_ref()
                        .context("submit_delivery requires an active executor subagent")?;
                    let committed = coordinator
                        .commit_delivery(
                            subagent,
                            &context.workspace_root,
                            &arguments.head_commit,
                            &arguments.verification_summary,
                        )
                        .await?;
                    if let Some(runtime) = runtime
                        && let Err(error) = runtime.publish_product_phase(
                            crate::studio::agent_host::root_agent_id(&session_id),
                            pl_core::AgentId::new(committed.agent_id.clone())?,
                            format!("delivery:{}", committed.outcome_id),
                            "deliveryCompleted".to_string(),
                            Some(format!(
                                "executor delivery committed for task {}",
                                committed.task_run_id
                            )),
                        )
                    {
                        tracing::warn!(
                            task_run_id = %committed.task_run_id,
                            %error,
                            "delivery product signal will be recovered from durable facts"
                        );
                    }
                    ToolExecutionResult::<serde_json::Value>::json(committed.delivery)
                        .map_err(anyhow::Error::from)
                }
            },
        )
        .with_effect(ToolEffect::BranchControl)
    }

    async fn validate_delivery<'a>(
        &self,
        validation: DeliveryValidation<'a>,
    ) -> Result<AgentDelivery> {
        let DeliveryValidation {
            scope,
            subagent,
            caller_workspace,
            supplied_head,
            verification_summary,
        } = validation;
        let run = &scope.run;
        let outcome = &scope.outcome;
        let work_unit = &scope.work_unit;
        if subagent.role != "executor" || outcome.role != "executor" {
            bail!("submit_delivery may only be called by the assigned executor");
        }
        let owner_path = subagent
            .parent_id
            .as_deref()
            .context("executor has no owner path")?;
        if outcome.owner_path != owner_path || outcome.task_run_id != run.id {
            bail!("executor does not own this task outcome");
        }
        if work_unit.task_run_id != run.id
            || work_unit.agent_id.as_deref() != Some(subagent.id.as_str())
        {
            bail!("executor does not own this work unit");
        }
        if !(1..=3).contains(&outcome.attempt)
            || !(1..=3).contains(&work_unit.attempt)
            || outcome.attempt != work_unit.attempt
        {
            bail!("delivery attempt must be within 1..=3 and match the work unit");
        }
        let verification_summary = verification_summary.trim();
        if verification_summary.is_empty() {
            bail!("verificationSummary must not be empty");
        }
        let supplied_head = supplied_head.trim();
        if supplied_head.is_empty() {
            bail!("headCommit must not be empty");
        }

        let snapshot = inspect_repository(caller_workspace, true).await?;
        if normalized_path(&snapshot.git_common_dir)
            != normalized_path(Path::new(&run.git_common_dir))
        {
            bail!("executor worktree does not belong to the active task repository");
        }
        if normalized_path(caller_workspace) != normalized_path(Path::new(&work_unit.worktree_path))
            || snapshot.branch != work_unit.branch
        {
            bail!(
                "caller workspace and branch do not match the assigned worktree {} on branch {}",
                work_unit.worktree_path,
                work_unit.branch
            );
        }
        if snapshot.head != supplied_head {
            bail!(
                "supplied headCommit {supplied_head} does not match worktree HEAD {}",
                snapshot.head
            );
        }
        let base_commit = work_unit.base_commit.as_str();
        if snapshot.head == base_commit {
            bail!("delivery HEAD must advance beyond base commit {base_commit}");
        }
        if !is_ancestor(&snapshot.workspace_root, base_commit, &snapshot.head).await? {
            bail!("delivery HEAD must descend from base commit {base_commit}");
        }
        let changed_files =
            changed_files_between(&snapshot.workspace_root, base_commit, &snapshot.head).await?;
        validate_owned_paths(&work_unit.owned_paths, &changed_files)?;

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

fn ensure_delivery_scope_is_open(scope: &DeliveryScope) -> Result<()> {
    if scope.outcome.status == AgentOutcomeStatus::Completed
        || scope.work_unit.status == WorkUnitStatus::Delivered
    {
        bail!("delivery is already finalized");
    }
    if !matches!(
        scope.outcome.status,
        AgentOutcomeStatus::Running | AgentOutcomeStatus::WaitingForDelivery
    ) || !matches!(
        scope.work_unit.status,
        WorkUnitStatus::Running | WorkUnitStatus::WaitingForDelivery
    ) {
        bail!("delivery scope is not accepting a delivery");
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
