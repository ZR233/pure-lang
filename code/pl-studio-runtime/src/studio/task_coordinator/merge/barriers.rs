#[cfg(test)]
use super::accept::MergeCommitTestBarrier;
use std::path::Path;

use super::git::{GitCommandOutput, checked_git, run_git};
use crate::studio::task_coordinator::{MergeRecord, TaskCoordinator, TaskMergeScope};

impl TaskCoordinator {
    pub(super) async fn mark_task_merge_verifying(
        &self,
        scope: &TaskMergeScope,
    ) -> anyhow::Result<MergeRecord> {
        self.store.mark_task_merge_verifying(&scope.merge.id).await
    }

    pub(super) async fn read_merge_index_tree(&self, workspace: &Path) -> anyhow::Result<String> {
        checked_git(workspace, vec!["write-tree".into()]).await
    }

    pub(super) async fn run_merge_commit(
        &self,
        workspace: &Path,
        message: String,
    ) -> anyhow::Result<GitCommandOutput> {
        let output = run_git(workspace, vec!["commit".into(), "-m".into(), message]).await?;
        Ok(output)
    }

    pub(super) async fn read_post_commit_head(&self, workspace: &Path) -> anyhow::Result<String> {
        checked_git(workspace, vec!["rev-parse".into(), "HEAD".into()]).await
    }

    pub(super) async fn read_accepted_task_run(
        &self,
        task_run_id: &str,
    ) -> anyhow::Result<Option<crate::studio::task_coordinator::TaskRunRecord>> {
        self.store.read_task_run(task_run_id).await
    }

    #[cfg(test)]
    pub(crate) fn set_merge_after_commit_barrier(&self, barrier: MergeCommitTestBarrier) {
        *self
            .merge_after_commit_barrier
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(barrier);
    }

    #[cfg(test)]
    pub(super) async fn pause_after_merge_commit(&self) {
        let barrier = self
            .merge_after_commit_barrier
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(barrier) = barrier {
            barrier.pause().await;
        }
    }

    #[cfg(not(test))]
    pub(super) async fn pause_after_merge_commit(&self) {}
}
