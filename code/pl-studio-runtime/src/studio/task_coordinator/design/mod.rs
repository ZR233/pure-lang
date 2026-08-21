mod cancel;
mod git;

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use futures::FutureExt;
use schemars::JsonSchema;
use serde::Deserialize;

use self::git::*;
use super::git::{ensure_no_git_operation, fingerprint_repository, inspect_repository};
use super::{
    BranchLeaseRecord, DesignFinalizeOutput, TaskCoordinator, TaskGitFingerprint, TaskRun,
    TaskRunStateKind,
};
use crate::ToolEffect;
use crate::tool::{FunctionToolDefinition, RegisteredTool, ToolExecutionResult};

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

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TaskFinalizeDesignInput {
    /// Non-empty summary of the design-stage conclusions and any workspace draft.
    summary: String,
}

impl TaskCoordinator {
    pub(crate) fn design_tool_completion_callback(
        self: &Arc<Self>,
        task_run_id: String,
        turn_id: String,
        workspace: std::path::PathBuf,
    ) -> pl_core::ToolCompletionCallback {
        let coordinator = self.clone();
        Arc::new(move |completion| {
            let coordinator = coordinator.clone();
            let task_run_id = task_run_id.clone();
            let turn_id = turn_id.clone();
            let workspace = workspace.clone();
            async move {
                // Finalize performs its own exact-scope validation. If it fails, its
                // post-hook must not turn an untrusted concurrent edit into an accepted
                // design observation.
                if completion.name == "task_finalize_design" {
                    return Ok(());
                }
                coordinator
                    .observe_design_workspace(
                        &task_run_id,
                        &turn_id,
                        &completion.call_id,
                        &workspace,
                    )
                    .await
            }
            .boxed()
        })
    }

    async fn observe_design_workspace(
        &self,
        task_run_id: &str,
        turn_id: &str,
        tool_call_id: &str,
        workspace: &Path,
    ) -> Result<()> {
        let Some(run) = self.store.read_task_run(task_run_id).await? else {
            return Ok(());
        };
        if run.kind() != TaskRunStateKind::DesignUpdating {
            return Ok(());
        }
        if normalized_path(workspace) != normalized_path(Path::new(&run.workspace_root)) {
            bail!("design tool observation workspace no longer matches its TaskRun");
        }
        let fingerprint =
            fingerprint_repository(workspace, &run.base_commit, &run.expected_head).await?;
        self.store
            .record_task_design_observation(task_run_id, turn_id, tool_call_id, fingerprint)
            .await?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn set_design_after_commit_barrier(&self, barrier: DesignCommitTestBarrier) {
        *self
            .design_after_commit_barrier
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(barrier);
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

    pub(crate) fn task_finalize_design_tool(
        self: &Arc<Self>,
        root_thread_id: impl Into<String>,
    ) -> RegisteredTool {
        let coordinator = self.clone();
        let root_thread_id = root_thread_id.into();
        FunctionToolDefinition::<TaskFinalizeDesignInput>::new(
            "task_finalize_design",
            "Finish the mandatory Task design stage after exploration and optional ordinary workspace edits. No design document change is required. If the workspace changed, the runtime commits the complete Task-owned draft before entering implementing; otherwise it advances without creating a commit.",
        )
        .registered(move |arguments: TaskFinalizeDesignInput, context| {
            let coordinator = coordinator.clone();
            let root_thread_id = root_thread_id.clone();
            async move {
                let output = coordinator
                    .finalize_design(
                        &root_thread_id,
                        context.workspace.root(),
                        &arguments.summary,
                    )
                    .await
                    .map_err(|error| anyhow::anyhow!("task_finalize_design failed: {error:#}"))?;
                ToolExecutionResult::<serde_json::Value>::json(output)
                    .map_err(anyhow::Error::from)
            }
        })
        .with_effect(ToolEffect::BranchControl)
    }

    pub(crate) async fn finalize_design(
        &self,
        root_thread_id: &str,
        caller_workspace: &Path,
        summary: &str,
    ) -> Result<DesignFinalizeOutput> {
        let summary = summary.trim();
        if summary.is_empty() {
            bail!("task_finalize_design summary must not be empty");
        }

        let _mutation_guard = self.lock_branch_mutation().await;
        let (run, _lease) = self
            .load_mutation_scope(root_thread_id, caller_workspace)
            .await?;
        ensure_design_phase(run.kind())?;

        let workspace = Path::new(&run.workspace_root);
        let draft_fingerprint =
            fingerprint_repository(workspace, &run.base_commit, &run.expected_head).await?;
        let observed_fingerprint = &run
            .latest_design_observation()
            .context("designUpdating state is missing its workspace observation")?
            .fingerprint;
        if observed_fingerprint != &draft_fingerprint {
            bail!(
                "workspace changed outside the Task tool observation chain; preserve the files and run another model tool before task_finalize_design"
            );
        }
        let changed_files = collect_worktree_changes(workspace).await?;
        if changed_files.is_empty() {
            return self
                .finalize_clean_design(&run, summary, &draft_fingerprint)
                .await;
        }
        self.finalize_design_draft(&run, summary, changed_files, draft_fingerprint)
            .await
    }

    async fn finalize_clean_design(
        &self,
        run: &TaskRun,
        summary: &str,
        fingerprint: &TaskGitFingerprint,
    ) -> Result<DesignFinalizeOutput> {
        match self
            .store
            .finalize_task_design(
                &run.id,
                &run.expected_head,
                &run.expected_head,
                None,
                summary,
                fingerprint,
            )
            .await
        {
            Ok(true) => {
                self.verify_durable_exact_scope(
                    run,
                    &run.expected_head,
                    "design finalization durable CAS",
                )
                .await?;
                Ok(DesignFinalizeOutput {
                    task_run_id: run.id.clone(),
                    previous_head: run.expected_head.clone(),
                    finalized_head: run.expected_head.clone(),
                    phase_commit: None,
                    changed_files: Vec::new(),
                    state: TaskRunStateKind::Implementing,
                })
            }
            Ok(false) => bail!("task head changed while finalizing the design stage"),
            Err(error) => Err(error).context("failed to finalize the design stage"),
        }
    }

    async fn finalize_design_draft(
        &self,
        run: &TaskRun,
        summary: &str,
        changed_files: Vec<String>,
        draft_fingerprint: TaskGitFingerprint,
    ) -> Result<DesignFinalizeOutput> {
        let workspace = Path::new(&run.workspace_root);
        let original_index_tree = write_tree(workspace).await?;
        let commit_result = async {
            stage_paths(workspace, &changed_files).await?;
            let staged = cached_changed_files(workspace).await?;
            ensure_same_paths(&staged, &changed_files)?;
            let remaining = collect_unstaged_and_untracked(workspace).await?;
            if !remaining.is_empty() {
                bail!(
                    "workspace changed concurrently while finalizing the design stage: {}",
                    remaining.join(", ")
                );
            }
            let validated_tree = write_tree(workspace).await?;
            run_git_checked(workspace, &["commit", "-m", "chore(task): 完成设计阶段"])
                .await
                .context("failed to create the design-stage baseline commit")?;
            let phase_commit = read_head(workspace).await?;
            Ok::<_, anyhow::Error>((phase_commit, validated_tree))
        }
        .await;
        let (phase_commit, validated_tree) = match commit_result {
            Ok(result) => result,
            Err(operation_error) => {
                self.restore_draft_index_or_block(
                    run,
                    &draft_fingerprint,
                    &original_index_tree,
                    &format!("design finalization failed: {operation_error}"),
                )
                .await?;
                return Err(operation_error).context(
                    "task_finalize_design did not create a baseline commit; the workspace draft was preserved",
                );
            }
        };

        #[cfg(test)]
        self.wait_after_design_commit().await;
        let exact_changed_files = match self
            .validate_captured_branch_commit(
                run,
                &phase_commit,
                &run.expected_head,
                &changed_files,
                &validated_tree,
            )
            .await
        {
            Ok(exact) => exact,
            Err(error) => {
                self.block_run(
                    run,
                    format!("design-stage commit could not be proven exact: {error}"),
                )
                .await?;
                return Err(error).context("design-stage commit could not be proven exact");
            }
        };
        self.ensure_exact_commit_is_clean(
            run,
            &phase_commit,
            "design-stage post-commit inspection failed",
        )
        .await?;

        match self
            .store
            .finalize_task_design(
                &run.id,
                &run.expected_head,
                &phase_commit,
                Some(&phase_commit),
                summary,
                &draft_fingerprint,
            )
            .await
        {
            Ok(true) => {
                self.verify_durable_exact_scope(
                    run,
                    &phase_commit,
                    "design finalization durable CAS",
                )
                .await?;
                Ok(DesignFinalizeOutput {
                    task_run_id: run.id.clone(),
                    previous_head: run.expected_head.clone(),
                    finalized_head: phase_commit.clone(),
                    phase_commit: Some(phase_commit),
                    changed_files: exact_changed_files,
                    state: TaskRunStateKind::Implementing,
                })
            }
            Ok(false) => {
                self.compensate_draft_commit_or_block(
                    run,
                    &draft_fingerprint,
                    &original_index_tree,
                    &phase_commit,
                    "design finalization head CAS failed",
                )
                .await?;
                bail!("task head changed while recording design finalization")
            }
            Err(error) => {
                self.compensate_draft_commit_or_block(
                    run,
                    &draft_fingerprint,
                    &original_index_tree,
                    &phase_commit,
                    &format!("design finalization transaction failed: {error}"),
                )
                .await?;
                Err(error).context("failed to record design finalization")
            }
        }
    }

    async fn load_mutation_scope(
        &self,
        root_thread_id: &str,
        caller_workspace: &Path,
    ) -> Result<(TaskRun, BranchLeaseRecord)> {
        let run = self
            .store
            .read_active_task_run_for_root_thread(root_thread_id)
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
        run: &TaskRun,
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
        let snapshot = inspect_repository(caller_workspace, false).await?;
        if normalized_path(&snapshot.workspace_root)
            != normalized_path(Path::new(&run.workspace_root))
            || normalized_path(&snapshot.git_common_dir)
                != normalized_path(Path::new(&run.git_common_dir))
            || snapshot.branch != run.branch
            || snapshot.head != run.expected_head
        {
            bail!("current workspace, branch, or HEAD does not match the active Task run");
        }
        ensure_no_git_operation(caller_workspace).await
    }

    async fn validate_captured_branch_commit(
        &self,
        run: &TaskRun,
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
        run: &TaskRun,
        commit: &str,
        reason: &str,
    ) -> Result<()> {
        let workspace = Path::new(&run.workspace_root);
        match inspect_repository(workspace, true).await {
            Ok(snapshot) if exact_run_scope(run, commit).matches(&snapshot) => Ok(()),
            Ok(_) | Err(_) => {
                self.block_run(run, format!("{reason}; external Git state was preserved"))
                    .await?;
                bail!("{reason}; the task run was blocked and Git state was preserved")
            }
        }
    }

    async fn verify_durable_exact_scope(
        &self,
        run: &TaskRun,
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

    async fn restore_draft_index_or_block(
        &self,
        run: &TaskRun,
        expected_fingerprint: &TaskGitFingerprint,
        original_index_tree: &str,
        reason: &str,
    ) -> Result<()> {
        let workspace = Path::new(&run.workspace_root);
        let restored = async {
            let snapshot = inspect_repository(workspace, false).await?;
            if !exact_run_scope(run, &run.expected_head).matches(&snapshot) {
                bail!("repository identity or HEAD changed while restoring the design draft");
            }
            ensure_no_git_operation(workspace).await?;
            run_git_checked(workspace, &["read-tree", original_index_tree]).await?;
            let actual =
                fingerprint_repository(workspace, &run.base_commit, &run.expected_head).await?;
            if &actual != expected_fingerprint {
                bail!("workspace draft fingerprint changed during design finalization");
            }
            Ok::<_, anyhow::Error>(())
        }
        .await;
        if let Err(error) = restored {
            self.block_run(
                run,
                format!("{reason}; workspace draft restoration failed: {error}"),
            )
            .await?;
            bail!(
                "{reason}; task run was blocked because the workspace draft could not be restored: {error}"
            )
        }
        Ok(())
    }

    async fn compensate_draft_commit_or_block(
        &self,
        run: &TaskRun,
        expected_fingerprint: &TaskGitFingerprint,
        original_index_tree: &str,
        commit: &str,
        reason: &str,
    ) -> Result<()> {
        let workspace = Path::new(&run.workspace_root);
        let safe = inspect_repository(workspace, true)
            .await
            .is_ok_and(|snapshot| exact_run_scope(run, commit).matches(&snapshot));
        if safe {
            let compensation = async {
                run_git_checked(workspace, &["reset", "--mixed", &run.expected_head]).await?;
                run_git_checked(workspace, &["read-tree", original_index_tree]).await?;
                let actual =
                    fingerprint_repository(workspace, &run.base_commit, &run.expected_head).await?;
                if &actual != expected_fingerprint {
                    bail!("compensated workspace does not match the original design draft");
                }
                Ok::<_, anyhow::Error>(())
            }
            .await;
            if let Err(error) = compensation {
                self.block_run(
                    run,
                    format!("{reason}; automatic compensation failed: {error}"),
                )
                .await?;
                bail!("{reason}; compensation failed and the task run was blocked: {error}")
            }
            return Ok(());
        }

        self.block_run(
            run,
            format!("{reason}; compensation was unsafe because HEAD or workspace changed"),
        )
        .await?;
        bail!("{reason}; task run was blocked because compensation was unsafe")
    }
}

fn exact_run_scope<'a>(run: &'a TaskRun, commit: &'a str) -> ExactRepositoryScope<'a> {
    ExactRepositoryScope {
        workspace_root: Path::new(&run.workspace_root),
        git_common_dir: Path::new(&run.git_common_dir),
        branch: &run.branch,
        head: commit,
    }
}

fn ensure_design_phase(phase: TaskRunStateKind) -> Result<()> {
    if phase == TaskRunStateKind::DesignUpdating {
        return Ok(());
    }
    bail!(
        "task_finalize_design requires phase designUpdating; current phase is {}",
        phase.as_str()
    )
}

fn ensure_same_paths(actual: &[String], expected: &[String]) -> Result<()> {
    let actual = actual.iter().collect::<BTreeSet<_>>();
    let expected = expected.iter().collect::<BTreeSet<_>>();
    if actual != expected {
        bail!(
            "staged design-stage paths do not match the workspace draft: expected {expected:?}, actual {actual:?}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests;
