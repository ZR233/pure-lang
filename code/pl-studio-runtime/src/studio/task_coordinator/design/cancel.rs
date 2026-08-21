use std::path::Path;

use anyhow::{Context, Result, bail};

use super::git::*;
use super::{TaskCoordinator, exact_run_scope};
use crate::studio::task_coordinator::git::{changed_files_between, inspect_repository};
use crate::studio::task_coordinator::{BranchMutationGuard, DesignCancellationRevert};

impl TaskCoordinator {
    #[cfg(test)]
    pub(crate) async fn revert_design_for_no_source_cancel(
        &self,
        task_run_id: &str,
    ) -> Result<DesignCancellationRevert> {
        let guard = self.lock_branch_mutation().await;
        self.revert_design_for_no_source_cancel_locked(task_run_id, &guard)
            .await
    }

    pub(crate) async fn revert_design_for_no_source_cancel_locked(
        &self,
        task_run_id: &str,
        guard: &BranchMutationGuard<'_>,
    ) -> Result<DesignCancellationRevert> {
        self.ensure_branch_mutation_guard(guard)?;
        let run = self
            .store
            .read_task_run(task_run_id)
            .await?
            .context("task run not found")?;
        if run.kind().is_terminal() {
            bail!("cannot revert the design-stage commit for a terminal task run");
        }
        let phase_commit = run
            .design_phase_commit()
            .context("task run has no design-stage commit")?;
        if run.expected_head != phase_commit || run.design_finalized_head() != Some(phase_commit) {
            bail!("task branch contains commits after the design-stage commit");
        }
        if !self.store.list_merge_records(&run.id).await?.is_empty() {
            bail!("task run already has an accepted source merge");
        }
        let lease = self
            .store
            .read_branch_lease(&run.id)
            .await?
            .context("task branch lease not found")?;
        self.validate_mutation_snapshot(&run, &lease, Path::new(&run.workspace_root))
            .await?;
        ensure_repository_clean(Path::new(&run.workspace_root)).await?;
        ensure_single_parent(
            Path::new(&run.workspace_root),
            phase_commit,
            &run.base_commit,
        )
        .await?;
        let changed_paths = changed_files_between(
            Path::new(&run.workspace_root),
            &run.base_commit,
            phase_commit,
        )
        .await?;
        let reverted_tree = read_tree(Path::new(&run.workspace_root), &run.base_commit).await?;

        let workspace = Path::new(&run.workspace_root);
        let revert_result = async {
            run_git_checked(workspace, &["revert", "--no-commit", phase_commit])
                .await
                .context("failed to apply the design-stage revert")?;
            run_git_checked(
                workspace,
                &["commit", "-m", "revert(task): 撤销设计阶段提交"],
            )
            .await
            .context("failed to commit the design-stage revert")?;
            Ok::<_, anyhow::Error>(())
        }
        .await;
        if let Err(error) = revert_result {
            self.block_run(
                &run,
                format!("design-stage revert failed and Git state was preserved: {error}"),
            )
            .await?;
            return Err(error).context("design-stage revert failed; Git state was preserved");
        }
        let revert_commit = match read_head(workspace).await {
            Ok(commit) => commit,
            Err(error) => {
                self.block_run(
                    &run,
                    format!("revert commit identity could not be captured: {error}"),
                )
                .await?;
                return Err(error).context("revert commit identity could not be captured");
            }
        };
        #[cfg(test)]
        self.wait_after_design_commit().await;
        if let Err(error) = self
            .validate_captured_branch_commit(
                &run,
                &revert_commit,
                phase_commit,
                &changed_paths,
                &reverted_tree,
            )
            .await
        {
            self.block_run(
                &run,
                format!("revert commit could not be proven exact: {error}"),
            )
            .await?;
            return Err(error).context("revert commit could not be proven exact");
        }
        self.ensure_exact_commit_is_clean(
            &run,
            &revert_commit,
            "revert post-commit inspection failed",
        )
        .await?;
        match self
            .store
            .compare_and_set_task_head(&run.id, phase_commit, &revert_commit)
            .await
        {
            Ok(true) => {
                self.verify_durable_exact_scope(&run, &revert_commit, "design revert durable CAS")
                    .await?;
                Ok(DesignCancellationRevert {
                    task_run_id: run.id.clone(),
                    previous_head: phase_commit.to_string(),
                    revert_commit,
                })
            }
            Ok(false) => {
                self.compensate_revert_or_block(
                    &run,
                    &revert_commit,
                    phase_commit,
                    "revert head CAS failed",
                )
                .await?;
                bail!("task head changed while recording the design-stage revert")
            }
            Err(error) => {
                self.compensate_revert_or_block(
                    &run,
                    &revert_commit,
                    phase_commit,
                    &format!("revert transaction failed: {error}"),
                )
                .await?;
                Err(error).context("failed to record the design-stage revert")
            }
        }
    }

    async fn compensate_revert_or_block(
        &self,
        run: &crate::studio::task_coordinator::TaskRun,
        revert_commit: &str,
        previous_head: &str,
        reason: &str,
    ) -> Result<()> {
        let workspace = Path::new(&run.workspace_root);
        let scope = exact_run_scope(run, revert_commit);
        let safe = inspect_repository(workspace, true)
            .await
            .is_ok_and(|snapshot| scope.matches(&snapshot));
        if safe {
            if let Err(error) = compensate_commit(scope, previous_head).await {
                self.block_run(
                    run,
                    format!("{reason}; automatic revert compensation failed: {error}"),
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
