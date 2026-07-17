use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use super::git::changed_files_between;
use super::merge::{ProductionMergeVerifier, select_merge_verification_commands};
use super::{MergeStatus, TaskCoordinator, TaskRunPhase, TaskRunRecord};
use crate::tool::{
    RegisteredTool, ToolExecutionResult, ToolInputSchemaField, strict_tool_input_schema,
};
use crate::{AgentLifecycleState, AgentRuntimeHandle, ToolEffect};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CompleteTaskInput {}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StopTaskInput {
    reason: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskCompletionOutput {
    run: TaskRunRecord,
    verification: Vec<super::MergeVerificationStep>,
}

impl TaskCoordinator {
    pub(crate) fn task_complete_tool(
        self: &Arc<Self>,
        session_id: impl Into<String>,
    ) -> RegisteredTool {
        let coordinator = self.clone();
        let session_id = session_id.into();
        RegisteredTool::from_typed_fallible_execution_result(
            "task_complete",
            "Complete a fully merged, design-consistent and reviewer-approved task.",
            strict_tool_input_schema([]),
            move |_: CompleteTaskInput, _context| {
                let coordinator = coordinator.clone();
                let session_id = session_id.clone();
                async move {
                    let output = coordinator.complete_task(&session_id).await?;
                    ToolExecutionResult::<serde_json::Value>::json(output)
                        .map_err(anyhow::Error::from)
                }
            },
        )
        .with_effect(ToolEffect::BranchControl)
    }

    pub(crate) fn task_stop_tool(
        self: &Arc<Self>,
        session_id: impl Into<String>,
        runtime: AgentRuntimeHandle,
    ) -> RegisteredTool {
        let coordinator = self.clone();
        let session_id = session_id.into();
        RegisteredTool::from_typed_fallible_execution_result(
            "task_stop",
            "Stop the current task after safely settling agents and branch state.",
            strict_tool_input_schema([ToolInputSchemaField::required(
                "reason",
                serde_json::json!({"type":"string"}),
            )]),
            move |input: StopTaskInput, _context| {
                let coordinator = coordinator.clone();
                let session_id = session_id.clone();
                let runtime = runtime.clone();
                async move {
                    let reason = input.reason.trim();
                    if reason.is_empty() {
                        bail!("task_stop reason must not be empty");
                    }
                    let run = {
                        let branch_guard = coordinator.lock_branch_mutation().await;
                        coordinator.ensure_branch_mutation_guard(&branch_guard)?;
                        let _allocation_guard = coordinator.allocation_lock.lock().await;
                        let run = coordinator
                            .preflight_task_stop_locked(&session_id, &branch_guard)
                            .await?;
                        coordinator
                            .store
                            .begin_task_stop(&run.id, &run.expected_head)
                            .await?
                    };
                    close_task_children(&runtime, &session_id).await?;
                    coordinator
                        .store
                        .settle_agents_for_task_stop(&run.id, reason)
                        .await?;
                    let branch_guard = coordinator.lock_branch_mutation().await;
                    let stopped = coordinator
                        .stop_task_locked(&run.id, reason, &branch_guard)
                        .await?;
                    ToolExecutionResult::<serde_json::Value>::json(stopped)
                        .map_err(anyhow::Error::from)
                }
            },
        )
        .with_effect(ToolEffect::BranchControl)
    }

    async fn complete_task(&self, session_id: &str) -> Result<TaskCompletionOutput> {
        let guard = self.lock_branch_mutation().await;
        self.ensure_branch_mutation_guard(&guard)?;
        let run = self
            .store
            .read_active_task_run_for_session(session_id)
            .await?;
        if run.phase != TaskRunPhase::Reviewing {
            bail!("task_complete requires reviewing phase");
        }
        self.ensure_process_lease_owned(&run)?;
        super::review::validate_review_repository(&run).await?;
        let changed_files = changed_files_between(
            Path::new(&run.workspace_root),
            &run.base_commit,
            &run.expected_head,
        )
        .await?;
        let commands =
            select_merge_verification_commands(Path::new(&run.workspace_root), &changed_files);
        let verification = ProductionMergeVerifier::verify_commands(commands).await;
        if verification.iter().any(|step| !step.success) {
            bail!("task completion verification failed");
        }
        let summary = if verification.is_empty() {
            "task completed; no additional final checks were required".to_string()
        } else {
            format!(
                "task completed after {} final verification checks",
                verification.len()
            )
        };
        let completed = self
            .store
            .complete_reviewed_task(session_id, &run.expected_head, &summary)
            .await?;
        self.release_owned_process_lease(&run.id);
        Ok(TaskCompletionOutput {
            run: completed,
            verification,
        })
    }

    async fn preflight_task_stop_locked(
        &self,
        session_id: &str,
        guard: &super::BranchMutationGuard<'_>,
    ) -> Result<TaskRunRecord> {
        self.ensure_branch_mutation_guard(guard)?;
        let run = self
            .store
            .read_active_task_run_for_session(session_id)
            .await?;
        self.validate_stoppable_run(&run).await?;
        Ok(run)
    }

    async fn stop_task_locked(
        &self,
        task_run_id: &str,
        reason: &str,
        guard: &super::BranchMutationGuard<'_>,
    ) -> Result<TaskRunRecord> {
        self.ensure_branch_mutation_guard(guard)?;
        let mut run = self
            .store
            .read_task_run(task_run_id)
            .await?
            .context("task run not found while stopping")?;
        self.validate_stoppable_run(&run).await?;
        let has_source_merge = self
            .store
            .list_merge_records(&run.id)
            .await?
            .iter()
            .any(|record| record.status == MergeStatus::Merged);
        if !has_source_merge {
            if run.design_commit.is_some() {
                self.revert_design_for_no_source_cancel_locked(&run.id, guard)
                    .await?;
                run = self
                    .store
                    .read_task_run(&run.id)
                    .await?
                    .context("task run disappeared after design revert")?;
            }
        } else if run.design_commit.as_deref() != Some(run.expected_head.as_str()) {
            bail!("task_stop requires a final design consistency update after source merges");
        }
        super::review::validate_review_repository(&run).await?;
        let cancelled = self
            .store
            .cancel_task_and_release_lease(&run.id, &run.expected_head, reason)
            .await?;
        self.release_owned_process_lease(&run.id);
        Ok(cancelled)
    }

    async fn validate_stoppable_run(&self, run: &TaskRunRecord) -> Result<()> {
        self.ensure_process_lease_owned(run)?;
        if matches!(
            run.phase,
            TaskRunPhase::Merging | TaskRunPhase::ResolvingConflict
        ) {
            bail!("task_stop requires the active merge or conflict to be aborted first");
        }
        super::review::validate_review_repository(run).await?;
        let merges = self.store.list_merge_records(&run.id).await?;
        if merges.iter().any(|record| {
            matches!(
                record.status,
                MergeStatus::Pending | MergeStatus::Verifying | MergeStatus::Conflicted
            )
        }) {
            bail!("task_stop requires all merge state to be settled");
        }
        Ok(())
    }
}

async fn close_task_children(runtime: &AgentRuntimeHandle, session_id: &str) -> Result<()> {
    let root = crate::studio::agent_host::root_agent_id(session_id);
    let children = runtime
        .list()
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()))?
        .into_iter()
        .filter(|snapshot| snapshot.identity.parent_id.as_ref() == Some(&root))
        .filter(|snapshot| {
            !matches!(
                snapshot.lifecycle,
                AgentLifecycleState::Closing | AgentLifecycleState::Closed
            )
        })
        .map(|snapshot| snapshot.identity.id)
        .collect::<Vec<_>>();
    for child in children {
        runtime
            .close(child)
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    }
    Ok(())
}
