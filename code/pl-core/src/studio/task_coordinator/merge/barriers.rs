#[cfg(test)]
use super::{MergeCleanupTestBarrier, accept::MergeCommitTestBarrier};
use crate::studio::task_coordinator::TaskCoordinator;

impl TaskCoordinator {
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
