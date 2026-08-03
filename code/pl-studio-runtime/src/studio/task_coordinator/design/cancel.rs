use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result, bail};

use super::git::*;
use super::patch::ensure_only_validated_design_changes;
use super::{TaskCoordinator, design_commit_is_current};
use crate::studio::task_coordinator::git::changed_files_between;
use crate::studio::task_coordinator::{BranchMutationGuard, DesignCancellationRevert, MergeStatus};

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
        let design_paths = changed_files_between(
            Path::new(&run.workspace_root),
            &run.base_commit,
            design_commit,
        )
        .await?;
        ensure_only_validated_design_changes(&design_paths, &design_paths)?;
        let reverted_tree = read_tree(Path::new(&run.workspace_root), &run.base_commit).await?;

        let workspace = Path::new(&run.workspace_root);
        let revert_result = async {
            run_git_checked(workspace, &["revert", "--no-commit", design_commit])
                .await
                .context("failed to apply the focused design revert")?;
            run_git_checked(workspace, &["commit", "-m", "撤销任务设计提交"])
                .await
                .context("failed to commit the focused design revert")?;
            Ok::<_, anyhow::Error>(())
        }
        .await;
        if let Err(error) = revert_result {
            self.block_run(
                &run,
                format!("design revert failed and Git state was preserved: {error}"),
            )
            .await?;
            return Err(error).context("design revert failed; Git state was preserved");
        }
        let revert_commit = match read_head(Path::new(&run.workspace_root)).await {
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
        let _reverted_paths = match self
            .validate_captured_branch_commit(
                &run,
                &revert_commit,
                design_commit,
                &design_paths,
                &reverted_tree,
            )
            .await
        {
            Ok(exact) => exact,
            Err(error) => {
                self.block_run(
                    &run,
                    format!("revert commit could not be proven exact: {error}"),
                )
                .await?;
                return Err(error).context("revert commit could not be proven exact");
            }
        };
        self.ensure_exact_commit_is_clean(
            &run,
            &BTreeMap::new(),
            &revert_commit,
            "revert post-commit inspection failed",
        )
        .await?;
        match self
            .store
            .compare_and_set_task_head(&run.id, design_commit, &revert_commit)
            .await
        {
            Ok(true) => {
                self.verify_durable_exact_scope(&run, &revert_commit, "design revert durable CAS")
                    .await?;
                Ok(DesignCancellationRevert {
                    task_run_id: run.id,
                    previous_head: design_commit.to_string(),
                    revert_commit,
                })
            }
            Ok(false) => {
                self.compensate_or_block(
                    &run,
                    &BTreeMap::new(),
                    &revert_commit,
                    "revert head CAS failed",
                )
                .await?;
                bail!("task head changed while recording the design revert")
            }
            Err(error) => {
                self.compensate_or_block(
                    &run,
                    &BTreeMap::new(),
                    &revert_commit,
                    &format!("revert transaction failed: {error}"),
                )
                .await?;
                Err(error).context("failed to record the design revert")
            }
        }
    }
}
