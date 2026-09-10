//! Retained unpublished product resources and serialized cleanup outside registry locks.
use super::{ThreadAssemblyError, children::ChildFactory};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Debug, Default)]
pub(super) struct CleanupGate {
    busy: AtomicBool,
    changed: tokio::sync::Notify,
}

pub(super) struct CleanupPermit(Arc<CleanupGate>);
impl Drop for CleanupPermit {
    fn drop(&mut self) {
        self.0.busy.store(false, Ordering::Release);
        self.0.changed.notify_waiters();
    }
}
impl CleanupGate {
    pub(super) async fn acquire(self: &Arc<Self>) -> CleanupPermit {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self
                .busy
                .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                return CleanupPermit(self.clone());
            }
            changed.await;
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct UnpublishedChild {
    pub(super) parent: String,
    pub(super) factory: ChildFactory,
    pub(super) cleanup: Arc<CleanupGate>,
}

impl super::StudioThreadAssembler {
    pub(super) fn publish_child_resources(&self, id: &str) -> Result<(), ThreadAssemblyError> {
        let mut state = self.0.state();
        if state.closing
            || state
                .preparing_children
                .get(id)
                .is_some_and(|child| child.cancellation.is_cancelled())
        {
            return Err(ThreadAssemblyError::Closed);
        }
        if !state.unpublished_children.contains_key(id) {
            return Err(ThreadAssemblyError::Identity(id.into()));
        }
        let entry = state
            .entries
            .get_mut(id)
            .ok_or_else(|| ThreadAssemblyError::Identity(id.into()))?;
        if entry.thread.snapshot().lifecycle != pl_core::thread::ThreadLifecycle::Open {
            return Err(ThreadAssemblyError::Closed);
        }
        entry.ready = true;
        state.unpublished_children.remove(id);
        Ok(())
    }

    /// Retries cleanup of a failed, unpublished child without rerunning its factory or tools.
    ///
    /// # Errors
    /// Returns the retained Thread close or product resource cleanup error.
    pub async fn discard_unpublished_child(&self, id: &str) -> Result<(), ThreadAssemblyError> {
        if self.0.state().preparing_children.contains_key(id) {
            return Err(ThreadAssemblyError::Preparing(id.into()));
        }
        self.discard_child_resources(id).await
    }

    pub(super) async fn discard_child_resources(
        &self,
        id: &str,
    ) -> Result<(), ThreadAssemblyError> {
        let pending = { self.0.state().unpublished_children.get(id).cloned() };
        let Some(pending) = pending else {
            return Ok(());
        };
        let _cleanup = pending.cleanup.acquire().await;
        if !self.0.state().unpublished_children.contains_key(id) {
            return Ok(());
        }
        self.close_thread(id).await?;
        pending.factory.discard_unpublished(id).await?;
        self.0.state().unpublished_children.remove(id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        ChildThreadRequest, StudioChildFactory, StudioThreadAssembler, StudioThreadSpec,
        tests::spec,
    };
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[derive(Debug, Clone)]
    struct CleanupProbe {
        failing: Arc<AtomicBool>,
        calls: Arc<AtomicUsize>,
    }
    impl StudioChildFactory for CleanupProbe {
        async fn prepare(
            &self,
            _request: ChildThreadRequest,
        ) -> Result<StudioThreadSpec, ThreadAssemblyError> {
            Err(ThreadAssemblyError::Closed)
        }
        async fn discard_unpublished(&self, _id: &str) -> Result<(), ThreadAssemblyError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            tokio::task::yield_now().await;
            if self.failing.load(Ordering::SeqCst) {
                Err(ThreadAssemblyError::Closed)
            } else {
                Ok(())
            }
        }
    }

    fn context() -> pl_core::tool::opaque::CallContext {
        pl_core::tool::opaque::CallContext {
            grant: Default::default(),
            thread_id: "root".into(),
            turn_id: "turn".into(),
            call_id: "spawn".into(),
            context: Default::default(),
            model_projection: None,
            tasks: None,
            cancellation: tokio_util::sync::CancellationToken::new(),
            extensions: Arc::new(Default::default()),
            catalog: Vec::new().into(),
            extension_sequence: 0,
        }
    }

    #[tokio::test]
    async fn failed_cleanup_retains_parent_and_retry_serializes_the_original_resource_owner() {
        let directory = tempfile::tempdir().unwrap();
        let probe = CleanupProbe {
            failing: Arc::new(AtomicBool::new(true)),
            calls: Arc::new(AtomicUsize::new(0)),
        };
        let owner = StudioThreadAssembler::default();
        owner.set_child_factory(probe.clone()).unwrap();
        let root = owner
            .assemble(spec("root", None, directory.path()))
            .await
            .unwrap();
        let reservation = owner.reserve_child(&context()).unwrap().1;
        let id = reservation.id.clone();
        assert!(matches!(
            owner.discard_unpublished_child(&id).await,
            Err(ThreadAssemblyError::Preparing(_))
        ));
        assert_eq!(probe.calls.load(Ordering::SeqCst), 0);
        drop(reservation);
        assert!(owner.discard_unpublished_child(&id).await.is_err());
        assert!(!owner.close_all().await.is_empty());
        assert!(owner.0.state().entries.contains_key("root"));
        assert!(owner.0.state().unpublished_children.contains_key(&id));
        let calls = probe.calls.load(Ordering::SeqCst);
        probe.failing.store(false, Ordering::SeqCst);
        let (first, second) = tokio::join!(
            owner.discard_unpublished_child(&id),
            owner.discard_unpublished_child(&id)
        );
        first.unwrap();
        second.unwrap();
        assert_eq!(
            probe.calls.load(Ordering::SeqCst),
            calls + 1,
            "concurrent retry must release the resource only once"
        );
        assert!(owner.close_all().await.is_empty());
        assert_eq!(
            root.snapshot().lifecycle,
            pl_core::thread::ThreadLifecycle::Closed
        );
        assert!(owner.0.state().unpublished_children.is_empty());
    }

    #[tokio::test]
    async fn assembled_child_stays_unpublished_until_product_resources_are_handed_over() {
        let directory = tempfile::tempdir().unwrap();
        let probe = CleanupProbe {
            failing: Arc::new(AtomicBool::new(false)),
            calls: Arc::new(AtomicUsize::new(0)),
        };
        let owner = StudioThreadAssembler::default();
        owner.set_child_factory(probe.clone()).unwrap();
        owner
            .assemble(spec("root", None, directory.path()))
            .await
            .unwrap();
        let reservation = owner.reserve_child(&context()).unwrap().1;
        let id = reservation.id.clone();
        owner
            .assemble(spec(&id, Some("root"), directory.path()))
            .await
            .unwrap();
        assert!(owner.thread(&id).is_none());
        owner.publish_child_resources(&id).unwrap();
        assert!(owner.thread(&id).is_some());
        drop(reservation);
        assert!(owner.close_all().await.is_empty());
        assert_eq!(
            probe.calls.load(Ordering::SeqCst),
            0,
            "published resources cannot be discarded as abandoned preparation"
        );
    }
}
