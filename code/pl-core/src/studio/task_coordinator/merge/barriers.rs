#[cfg(test)]
use super::{MergeCleanupTestBarrier, accept::MergeCommitTestBarrier};
use std::path::Path;

use super::git::{GitCommandOutput, checked_git, run_git};
use crate::studio::task_coordinator::{MergeRecord, TaskCoordinator, TaskMergeScope};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::studio::task_coordinator) enum MergeFailurePoint {
    MarkVerifying,
    WriteTree,
    CommitRunnerBeforeStart,
    CommitRunnerAfterSuccess,
    PostCommitRevParse,
    ConflictManifest,
    ConflictPersistence,
    FailurePersistence,
}

#[cfg(test)]
pub(in crate::studio::task_coordinator) type MergeFailureTestPoint = MergeFailurePoint;

impl TaskCoordinator {
    pub(super) async fn mark_task_merge_verifying(
        &self,
        scope: &TaskMergeScope,
    ) -> anyhow::Result<MergeRecord> {
        self.inject_merge_failure(MergeFailurePoint::MarkVerifying)?;
        self.store.mark_task_merge_verifying(&scope.merge.id).await
    }

    pub(super) async fn read_merge_index_tree(&self, workspace: &Path) -> anyhow::Result<String> {
        self.inject_merge_failure(MergeFailurePoint::WriteTree)?;
        checked_git(workspace, vec!["write-tree".into()]).await
    }

    pub(super) async fn run_merge_commit(
        &self,
        workspace: &Path,
        message: String,
    ) -> anyhow::Result<GitCommandOutput> {
        self.inject_merge_failure(MergeFailurePoint::CommitRunnerBeforeStart)?;
        let output = run_git(workspace, vec!["commit".into(), "-m".into(), message]).await?;
        self.inject_merge_failure(MergeFailurePoint::CommitRunnerAfterSuccess)?;
        Ok(output)
    }

    pub(super) async fn read_post_commit_head(&self, workspace: &Path) -> anyhow::Result<String> {
        self.inject_merge_failure(MergeFailurePoint::PostCommitRevParse)?;
        checked_git(workspace, vec!["rev-parse".into(), "HEAD".into()]).await
    }

    #[cfg(test)]
    pub(in crate::studio::task_coordinator) fn fail_next_merge_at(
        &self,
        point: MergeFailureTestPoint,
    ) {
        *self
            .merge_failure_point
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(point);
    }

    #[cfg(test)]
    pub(super) fn inject_merge_failure(&self, point: MergeFailurePoint) -> anyhow::Result<()> {
        let mut selected = self
            .merge_failure_point
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if selected.as_ref() == Some(&point) {
            selected.take();
            anyhow::bail!("injected merge failure at {point:?}");
        }
        Ok(())
    }

    #[cfg(not(test))]
    pub(super) fn inject_merge_failure(&self, _point: MergeFailurePoint) -> anyhow::Result<()> {
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn fail_next_merge_post_accept_read(&self) {
        self.fail_merge_post_accept_read
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    #[cfg(test)]
    pub(super) async fn read_accepted_task_run(
        &self,
        task_run_id: &str,
    ) -> anyhow::Result<Option<crate::studio::task_coordinator::TaskRunRecord>> {
        if self
            .fail_merge_post_accept_read
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            anyhow::bail!("injected accepted task run read failure");
        }
        self.store.read_task_run(task_run_id).await
    }

    #[cfg(not(test))]
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
        pause(&self.merge_after_commit_barrier).await;
    }

    #[cfg(not(test))]
    pub(super) async fn pause_after_merge_commit(&self) {}

    #[cfg(test)]
    pub(crate) fn set_merge_before_proof_barrier(&self, barrier: MergeCommitTestBarrier) {
        *self
            .merge_before_proof_barrier
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(barrier);
    }

    #[cfg(test)]
    pub(super) async fn pause_before_merge_proof(&self) {
        pause(&self.merge_before_proof_barrier).await;
    }

    #[cfg(not(test))]
    pub(super) async fn pause_before_merge_proof(&self) {}

    #[cfg(test)]
    pub(crate) fn set_merge_after_acceptance_barrier(&self, barrier: MergeCommitTestBarrier) {
        *self
            .merge_after_acceptance_barrier
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(barrier);
    }

    #[cfg(test)]
    pub(super) async fn pause_after_merge_acceptance(&self) {
        pause(&self.merge_after_acceptance_barrier).await;
    }

    #[cfg(not(test))]
    pub(super) async fn pause_after_merge_acceptance(&self) {}

    #[cfg(test)]
    pub(crate) fn set_merge_after_abort_barrier(&self, barrier: MergeCommitTestBarrier) {
        *self
            .merge_after_abort_barrier
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(barrier);
    }

    #[cfg(test)]
    pub(super) async fn pause_after_merge_abort(&self) {
        pause(&self.merge_after_abort_barrier).await;
    }

    #[cfg(not(test))]
    pub(super) async fn pause_after_merge_abort(&self) {}

    #[cfg(test)]
    pub(crate) fn set_merge_cleanup_barrier(&self, barrier: MergeCleanupTestBarrier) {
        *self
            .merge_cleanup_barrier
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(barrier);
    }

    #[cfg(test)]
    pub(super) async fn pause_before_merge_cleanup(&self) {
        pause(&self.merge_cleanup_barrier).await;
    }

    #[cfg(not(test))]
    pub(super) async fn pause_before_merge_cleanup(&self) {}
}

#[cfg(test)]
async fn pause<T>(slot: &std::sync::Mutex<Option<T>>)
where
    T: BarrierPause,
{
    let barrier = slot
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();
    if let Some(barrier) = barrier {
        barrier.pause().await;
    }
}

#[cfg(test)]
trait BarrierPause {
    fn pause(&self) -> impl std::future::Future<Output = ()> + Send;
}

#[cfg(test)]
impl BarrierPause for MergeCommitTestBarrier {
    async fn pause(&self) {
        self.pause().await;
    }
}

#[cfg(test)]
impl BarrierPause for MergeCleanupTestBarrier {
    async fn pause(&self) {
        self.pause().await;
    }
}
