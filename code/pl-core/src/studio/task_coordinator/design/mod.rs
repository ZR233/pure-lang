mod git;
mod patch;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use self::git::*;
use self::patch::*;
use super::git::inspect_repository;
use super::{
    BranchLeaseRecord, DesignCancellationRevert, DesignUpdateOutput, MergeStatus, TaskCoordinator,
    TaskRunPhase, TaskRunRecord,
};
use crate::tool::{
    LocalWorkspaceFileBackend, RegisteredTool, ToolExecutionResult, ToolInputSchemaField,
    apply_patch_to_backend, strict_tool_input_schema,
};
use crate::turn::{CompileMode, ToolEffect, TurnExecutionRole};

#[cfg(test)]
#[derive(Clone)]
pub(crate) struct DesignCommitTestBarrier {
    committed: Arc<tokio::sync::Barrier>,
    release: Arc<tokio::sync::Barrier>,
}

#[cfg(test)]
impl DesignCommitTestBarrier {
    pub(crate) fn new() -> Self {
        Self {
            committed: Arc::new(tokio::sync::Barrier::new(2)),
            release: Arc::new(tokio::sync::Barrier::new(2)),
        }
    }

    pub(crate) async fn wait_until_committed(&self) {
        self.committed.wait().await;
    }

    pub(crate) async fn release(&self) {
        self.release.wait().await;
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskUpdateDesignInput {
    patch: String,
}

#[derive(Debug)]
struct ValidatedDesignPatch {
    patch: String,
    paths: Vec<String>,
}

#[derive(Debug)]
enum OriginalPath {
    Missing,
    File(Vec<u8>),
}

impl TaskCoordinator {
    #[cfg(test)]
    pub(crate) fn set_design_after_commit_barrier(&self, barrier: DesignCommitTestBarrier) {
        *self
            .design_after_commit_barrier
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(barrier);
    }

    #[cfg(test)]
    pub(crate) fn fail_design_compensation_for_test(&self) {
        self.fail_design_compensation
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    #[cfg(test)]
    async fn wait_after_design_commit(&self) {
        let barrier = self
            .design_after_commit_barrier
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if let Some(barrier) = barrier {
            barrier.committed.wait().await;
            barrier.release.wait().await;
        }
    }

    pub(crate) fn task_update_design_tool(
        self: &Arc<Self>,
        studio_session_id: impl Into<String>,
    ) -> RegisteredTool {
        let coordinator = self.clone();
        let studio_session_id = studio_session_id.into();
        RegisteredTool::from_fallible_execution_result(
            "task_update_design",
            "Apply and commit a design-only patch for the current Task run.",
            strict_tool_input_schema([ToolInputSchemaField::required(
                "patch",
                serde_json::json!({ "type": "string" }),
            )]),
            move |input, context| {
                let coordinator = coordinator.clone();
                let studio_session_id = studio_session_id.clone();
                async move {
                    let arguments: TaskUpdateDesignInput = serde_json::from_value(input.arguments)
                        .context("invalid task_update_design input")?;
                    if context.mode != CompileMode::Task
                        || context.active_subagent.is_some()
                        || context.execution_profile().role() != TurnExecutionRole::Planner
                        || !context.execution_profile().is_root_owner()
                    {
                        bail!("task_update_design requires the root Task planner");
                    }
                    let output = coordinator
                        .update_design(
                            &studio_session_id,
                            &context.workspace_root,
                            &arguments.patch,
                        )
                        .await?;
                    ToolExecutionResult::<serde_json::Value>::json(output)
                        .map_err(anyhow::Error::from)
                }
            },
        )
        .with_effect(ToolEffect::BranchControl)
    }

    pub(crate) async fn update_design(
        &self,
        studio_session_id: &str,
        caller_workspace: &Path,
        patch: &str,
    ) -> Result<DesignUpdateOutput> {
        let _mutation_guard = self.branch_mutation_lock.lock().await;
        let (run, _lease) = self
            .load_mutation_scope(studio_session_id, caller_workspace)
            .await?;
        ensure_design_phase(run.phase)?;
        let validated = validate_design_patch(caller_workspace, patch).await?;
        let originals = snapshot_paths(caller_workspace, &validated.paths).await?;
        self.apply_and_commit_design(&run, validated, &originals)
            .await
    }

    pub(crate) async fn revert_design_for_no_source_cancel(
        &self,
        task_run_id: &str,
    ) -> Result<DesignCancellationRevert> {
        let _mutation_guard = self.branch_mutation_lock.lock().await;
        let run = self
            .store
            .read_task_run(task_run_id)
            .await?
            .context("task run not found")?;
        if run.phase.is_terminal() {
            bail!("cannot revert design for a terminal task run");
        }
        let design_commit = run
            .design_commit
            .as_deref()
            .context("task run has no accepted design commit")?;
        if !design_commit_is_current(&run) {
            bail!("task branch contains commits after the accepted design commit");
        }
        if self
            .store
            .list_merge_records(&run.id)
            .await?
            .iter()
            .any(|record| record.status == MergeStatus::Merged)
        {
            bail!("task run already has an accepted source merge");
        }
        let lease = self
            .store
            .read_branch_lease(&run.id)
            .await?
            .context("task branch lease not found")?;
        self.validate_mutation_snapshot(&run, &lease, Path::new(&run.workspace_root))
            .await?;
        ensure_single_parent(
            Path::new(&run.workspace_root),
            design_commit,
            &run.base_commit,
        )
        .await?;

        run_git_checked(
            Path::new(&run.workspace_root),
            &["revert", "--no-edit", design_commit],
        )
        .await?;
        let snapshot = inspect_repository(&run.workspace_root, true).await?;
        let revert_commit = snapshot.head;
        match self
            .store
            .compare_and_set_task_head(&run.id, design_commit, &revert_commit)
            .await
        {
            Ok(true) => Ok(DesignCancellationRevert {
                task_run_id: run.id,
                previous_head: design_commit.to_string(),
                revert_commit,
            }),
            Ok(false) => {
                compensate_commit(
                    Path::new(&run.workspace_root),
                    &revert_commit,
                    design_commit,
                )
                .await?;
                bail!("task head changed while recording the design revert")
            }
            Err(error) => {
                compensate_commit(
                    Path::new(&run.workspace_root),
                    &revert_commit,
                    design_commit,
                )
                .await?;
                Err(error).context("failed to record the design revert")
            }
        }
    }

    async fn load_mutation_scope(
        &self,
        studio_session_id: &str,
        caller_workspace: &Path,
    ) -> Result<(TaskRunRecord, BranchLeaseRecord)> {
        let run = self
            .store
            .read_active_task_run_for_session(studio_session_id)
            .await?;
        let lease = self
            .store
            .read_branch_lease(&run.id)
            .await?
            .context("task branch lease not found")?;
        self.validate_mutation_snapshot(&run, &lease, caller_workspace)
            .await?;
        Ok((run, lease))
    }

    async fn validate_mutation_snapshot(
        &self,
        run: &TaskRunRecord,
        lease: &BranchLeaseRecord,
        caller_workspace: &Path,
    ) -> Result<()> {
        self.ensure_process_lease_owned(run)?;
        if lease.task_run_id != run.id
            || normalized_path(Path::new(&lease.git_common_dir))
                != normalized_path(Path::new(&run.git_common_dir))
            || lease.branch != run.branch
            || lease.expected_head != run.expected_head
        {
            bail!("TaskRun and BranchLease no longer describe the same branch head");
        }
        let snapshot = inspect_repository(caller_workspace, true).await?;
        if normalized_path(&snapshot.workspace_root)
            != normalized_path(Path::new(&run.workspace_root))
            || normalized_path(&snapshot.git_common_dir)
                != normalized_path(Path::new(&run.git_common_dir))
            || snapshot.branch != run.branch
            || snapshot.head != run.expected_head
        {
            bail!("current workspace, branch, or HEAD does not match the active Task run");
        }
        Ok(())
    }

    async fn apply_and_commit_design(
        &self,
        run: &TaskRunRecord,
        validated: ValidatedDesignPatch,
        originals: &BTreeMap<String, OriginalPath>,
    ) -> Result<DesignUpdateOutput> {
        let workspace = Path::new(&run.workspace_root);
        let commit_result = async {
            let backend = LocalWorkspaceFileBackend::new(workspace.to_path_buf(), false).await?;
            apply_patch_to_backend(&backend, ".".to_string(), &validated.patch).await?;

            let changed = collect_worktree_changes(workspace).await?;
            if changed.is_empty() {
                bail!("task_update_design patch produced no changes");
            }
            ensure_only_validated_design_changes(&changed, &validated.paths)?;
            stage_paths(workspace, &validated.paths).await?;
            let changed_files = cached_changed_files(workspace).await?;
            if changed_files.is_empty() {
                bail!("task_update_design patch produced no staged changes");
            }
            ensure_only_validated_design_changes(&changed_files, &validated.paths)?;
            let remaining = collect_unstaged_and_untracked(workspace).await?;
            if !remaining.is_empty() {
                bail!("workspace changed concurrently while committing task design");
            }

            run_git_checked(workspace, &["commit", "-m", "更新任务设计"])
                .await
                .context("failed to create focused design commit")?;
            let committed = inspect_repository(workspace, true).await?;
            Ok::<_, anyhow::Error>((changed_files, committed.head))
        }
        .await;
        let (changed_files, design_commit) = match commit_result {
            Ok(committed) => committed,
            Err(operation_error) => {
                if let Err(cleanup_error) =
                    rollback_paths(workspace, originals, &run.expected_head).await
                {
                    bail!(
                        "design update failed: {operation_error}; rollback also failed: {cleanup_error}"
                    );
                }
                return Err(operation_error);
            }
        };

        #[cfg(test)]
        self.wait_after_design_commit().await;

        match self
            .store
            .advance_task_design_head(&run.id, &run.expected_head, &design_commit)
            .await
        {
            Ok(true) => Ok(DesignUpdateOutput {
                task_run_id: run.id.clone(),
                previous_head: run.expected_head.clone(),
                design_commit,
                changed_files,
            }),
            Ok(false) => {
                self.compensate_or_block(run, originals, &design_commit, "design head CAS failed")
                    .await?;
                bail!("task head changed while recording the design commit")
            }
            Err(error) => {
                self.compensate_or_block(
                    run,
                    originals,
                    &design_commit,
                    &format!("design transaction failed: {error}"),
                )
                .await?;
                Err(error).context("failed to record the design commit")
            }
        }
    }

    async fn compensate_or_block(
        &self,
        run: &TaskRunRecord,
        originals: &BTreeMap<String, OriginalPath>,
        commit: &str,
        reason: &str,
    ) -> Result<()> {
        let workspace = Path::new(&run.workspace_root);
        let safe = inspect_repository(workspace, true)
            .await
            .is_ok_and(|snapshot| snapshot.head == commit);
        if safe {
            #[cfg(test)]
            let compensation = if self
                .fail_design_compensation
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                Err(anyhow::anyhow!("injected design compensation failure"))
            } else {
                compensate_commit(workspace, commit, &run.expected_head).await
            };
            #[cfg(not(test))]
            let compensation = compensate_commit(workspace, commit, &run.expected_head).await;

            let cleanup = async {
                compensation?;
                restore_originals(workspace, originals).await?;
                ensure_repository_clean(workspace).await
            }
            .await;
            if let Err(cleanup_error) = cleanup {
                self.block_run(
                    run,
                    format!(
                        "{reason}; automatic compensation failed after the design commit: {cleanup_error}"
                    ),
                )
                .await?;
                bail!(
                    "{reason}; compensation failed and the task run was blocked: {cleanup_error}"
                );
            }
            return Ok(());
        }

        self.block_run(
            run,
            format!(
                "{reason}; automatic compensation was unsafe because HEAD or workspace changed"
            ),
        )
        .await?;
        bail!("{reason}; task run was blocked because compensation was unsafe")
    }
}

fn ensure_design_phase(phase: TaskRunPhase) -> Result<()> {
    if matches!(
        phase,
        TaskRunPhase::DesignUpdating | TaskRunPhase::Implementing | TaskRunPhase::Reworking
    ) {
        return Ok(());
    }
    bail!(
        "task_update_design is not allowed during phase {}",
        phase.as_str()
    )
}

pub(crate) fn design_commit_is_current(run: &TaskRunRecord) -> bool {
    run.design_commit.as_deref() == Some(run.expected_head.as_str())
}

#[cfg(test)]
mod tests;
