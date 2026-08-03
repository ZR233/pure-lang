mod cancel;
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
use super::{BranchLeaseRecord, DesignUpdateOutput, TaskCoordinator, TaskRunPhase, TaskRunRecord};
use crate::ToolEffect;
use crate::tool::{
    LocalWorkspaceFileBackend, RegisteredTool, ToolExecutionResult, ToolInputSchemaField,
    apply_patch_to_backend, strict_tool_input_schema,
};

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
    pub(crate) fn set_design_before_head_persist_barrier(&self, barrier: DesignCommitTestBarrier) {
        *self
            .design_before_head_persist_barrier
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(barrier);
    }

    #[cfg(test)]
    pub(crate) fn set_design_after_head_persist_barrier(&self, barrier: DesignCommitTestBarrier) {
        *self
            .design_after_head_persist_barrier
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(barrier);
    }

    #[cfg(test)]
    pub(crate) fn set_design_before_rollback_barrier(&self, barrier: DesignCommitTestBarrier) {
        *self
            .design_before_rollback_barrier
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

    #[cfg(test)]
    async fn wait_before_design_head_persist(&self) {
        let barrier = self
            .design_before_head_persist_barrier
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if let Some(barrier) = barrier {
            barrier.committed.wait().await;
            barrier.release.wait().await;
        }
    }

    #[cfg(test)]
    async fn wait_after_design_head_persist(&self) {
        let barrier = self
            .design_after_head_persist_barrier
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if let Some(barrier) = barrier {
            barrier.committed.wait().await;
            barrier.release.wait().await;
        }
    }

    #[cfg(test)]
    async fn wait_before_design_rollback(&self) {
        let barrier = self
            .design_before_rollback_barrier
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
            "Apply and commit one design-only Codex patch for the current Task run. The patch argument must contain exactly one complete block: one `*** Begin Patch` wrapper, one matching `*** End Patch` wrapper, and nothing outside them. Do not prepend a template, append another block, use Markdown fences, or include any previous failed attempt. The patch itself declares the changed design files; plan prose is reading context only. Use `*** Add File: design/<path>` for a new file and prefix every content line with `+`, without an `@@` hunk. Use `*** Update File:` only for an existing file. After failure, follow the reported cause, reread stale targets when needed, then replace the entire argument with one corrected block. Applied hunks from a failed call are rolled back. Never use `*** New File`.",
            strict_tool_input_schema([ToolInputSchemaField::required(
                "patch",
                serde_json::json!({
                    "type": "string",
                    "description": "Exactly one complete Codex patch block for design/**. Do not include prose, Markdown fences, templates, or a previous attempt."
                }),
            )]),
            move |input, context| {
                let coordinator = coordinator.clone();
                let studio_session_id = studio_session_id.clone();
                async move {
                    let arguments: TaskUpdateDesignInput = serde_json::from_value(input.arguments)
                        .context("invalid task_update_design input")?;
                    let output = coordinator
                        .update_design(
                            &studio_session_id,
                            &context.workspace_root,
                            &arguments.patch,
                        )
                        .await
                        .map_err(|error| anyhow::anyhow!("task_update_design failed: {error:#}"))?;
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
        let _mutation_guard = self.lock_branch_mutation().await;
        let (run, _lease) = self
            .load_mutation_scope(studio_session_id, caller_workspace)
            .await?;
        ensure_design_phase(run.phase)?;
        let validated = validate_design_patch(caller_workspace, patch).await?;
        let originals = snapshot_paths(caller_workspace, &validated.paths).await?;
        self.apply_and_commit_design(&run, validated, &originals)
            .await
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
            let validated_tree = write_tree(workspace).await?;

            run_git_checked(workspace, &["commit", "-m", "更新任务设计"])
                .await
                .context("failed to create focused design commit")?;
            Ok::<_, anyhow::Error>(validated_tree)
        }
        .await;
        let validated_tree = match commit_result {
            Ok(validated_tree) => validated_tree,
            Err(operation_error) => {
                #[cfg(test)]
                self.wait_before_design_rollback().await;
                if let Err(rollback_error) =
                    rollback_paths(workspace, originals, &run.expected_head).await
                {
                    self.block_run(
                        run,
                        format!(
                            "design update failed: {operation_error}; rollback could not safely restore the repository: {rollback_error}; Git state was preserved"
                        ),
                    )
                    .await?;
                    bail!(
                        "design update failed: {operation_error}; rollback could not safely restore the repository and the task run was blocked: {rollback_error}"
                    );
                }
                if let Err(cleanliness_error) = ensure_repository_clean(workspace).await {
                    self.block_run(
                        run,
                        format!(
                            "design update failed: {operation_error}; repository was not clean after rollback: {cleanliness_error}; residual Git state was preserved"
                        ),
                    )
                    .await?;
                    bail!(
                        "design update failed: {operation_error}; repository was not clean after rollback and the task run was blocked: {cleanliness_error}"
                    );
                }
                bail!(
                    "task_update_design did not record a design commit: {operation_error:#}; the coordinator restored the validated design paths and index to their pre-call state; retry with one complete logical patch"
                );
            }
        };

        let design_commit = match read_head(workspace).await {
            Ok(commit) => commit,
            Err(error) => {
                self.block_run(
                    run,
                    format!("design commit identity could not be captured: {error}"),
                )
                .await?;
                return Err(error).context("design commit identity could not be captured");
            }
        };
        #[cfg(test)]
        self.wait_after_design_commit().await;

        let changed_files = match self
            .validate_captured_branch_commit(
                run,
                &design_commit,
                &run.expected_head,
                &validated.paths,
                &validated_tree,
            )
            .await
        {
            Ok(exact) => exact,
            Err(error) => {
                self.block_run(
                    run,
                    format!("design commit could not be proven exact: {error}"),
                )
                .await?;
                return Err(error).context("design commit could not be proven exact");
            }
        };
        self.ensure_exact_commit_is_clean(
            run,
            originals,
            &design_commit,
            "design post-commit inspection failed",
        )
        .await?;
        #[cfg(test)]
        self.wait_before_design_head_persist().await;

        match self
            .store
            .advance_task_design_head(&run.id, &run.expected_head, &design_commit)
            .await
        {
            Ok(true) => {
                #[cfg(test)]
                self.wait_after_design_head_persist().await;
                self.verify_durable_exact_scope(run, &design_commit, "design commit durable CAS")
                    .await?;
                Ok(DesignUpdateOutput {
                    task_run_id: run.id.clone(),
                    previous_head: run.expected_head.clone(),
                    design_commit,
                    changed_files,
                })
            }
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

    async fn validate_captured_branch_commit(
        &self,
        run: &TaskRunRecord,
        commit: &str,
        previous_head: &str,
        expected_paths: &[String],
        expected_tree: &str,
    ) -> Result<Vec<String>> {
        let workspace = Path::new(&run.workspace_root);
        let changed_files = validate_exact_commit(
            workspace,
            commit,
            previous_head,
            expected_paths,
            expected_tree,
        )
        .await?;
        let snapshot = inspect_repository(workspace, false).await?;
        if !exact_run_scope(run, commit).matches(&snapshot) {
            bail!("current named branch no longer points to the exact task commit");
        }
        Ok(changed_files)
    }

    async fn ensure_exact_commit_is_clean(
        &self,
        run: &TaskRunRecord,
        originals: &BTreeMap<String, OriginalPath>,
        commit: &str,
        reason: &str,
    ) -> Result<()> {
        let workspace = Path::new(&run.workspace_root);
        match inspect_repository(workspace, true).await {
            Ok(snapshot) if exact_run_scope(run, commit).matches(&snapshot) => Ok(()),
            Ok(_) | Err(_) => {
                self.compensate_or_block(run, originals, commit, reason)
                    .await?;
                bail!("{reason}; exact commit was compensated")
            }
        }
    }

    async fn verify_durable_exact_scope(
        &self,
        run: &TaskRunRecord,
        commit: &str,
        operation: &str,
    ) -> Result<()> {
        let failure = match inspect_repository(Path::new(&run.workspace_root), true).await {
            Ok(snapshot) if exact_run_scope(run, commit).matches(&snapshot) => return Ok(()),
            Ok(snapshot) => format!(
                "expected branch {} at {commit}, found branch {} at {}",
                run.branch, snapshot.branch, snapshot.head
            ),
            Err(error) => error.to_string(),
        };
        self.block_run(
            run,
            format!(
                "{operation} advanced durable task and lease heads, but final exact repository scope verification failed: {failure}; external Git state was preserved"
            ),
        )
        .await?;
        bail!(
            "{operation} advanced durable heads, but final exact repository scope verification failed and the task run was blocked: {failure}"
        )
    }

    async fn compensate_or_block(
        &self,
        run: &TaskRunRecord,
        originals: &BTreeMap<String, OriginalPath>,
        commit: &str,
        reason: &str,
    ) -> Result<()> {
        let workspace = Path::new(&run.workspace_root);
        let scope = exact_run_scope(run, commit);
        let safe = inspect_repository(workspace, true)
            .await
            .is_ok_and(|snapshot| scope.matches(&snapshot));
        if safe {
            #[cfg(test)]
            let compensation = if self
                .fail_design_compensation
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                Err(anyhow::anyhow!("injected design compensation failure"))
            } else {
                compensate_commit(scope, &run.expected_head).await
            };
            #[cfg(not(test))]
            let compensation = compensate_commit(scope, &run.expected_head).await;

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

    pub(super) fn ensure_executor_design_contract(&self, run: &TaskRunRecord) -> Result<()> {
        if design_commit_is_current(run) {
            return Ok(());
        }
        bail!("task_spawn_executor requires a durable design commit at the current task HEAD")
    }
}

fn exact_run_scope<'a>(run: &'a TaskRunRecord, commit: &'a str) -> ExactRepositoryScope<'a> {
    ExactRepositoryScope {
        workspace_root: Path::new(&run.workspace_root),
        git_common_dir: Path::new(&run.git_common_dir),
        branch: &run.branch,
        head: commit,
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
