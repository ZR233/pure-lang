//! Published workspace cleanup remains owned until the frozen close policy succeeds.
use super::{StudioThreadAssembler, ThreadAssemblyError};
use pl_tool::collaboration::thread::AgentWorkspaceDisposition;
use std::sync::Arc;

impl StudioThreadAssembler {
    pub(super) fn seal_agent_close(
        &self,
        targets: &[(usize, String)],
        disposition: AgentWorkspaceDisposition,
    ) -> Result<(), ThreadAssemblyError> {
        let mut state = self.0.state();
        for (_, id) in targets {
            if let Some(entry) = state.entries.get(id)
                && entry
                    .workspace_disposition
                    .is_some_and(|current| current != disposition)
            {
                return Err(ThreadAssemblyError::CloseDisposition(id.clone()));
            }
        }
        for (_, id) in targets {
            if let Some(entry) = state.entries.get_mut(id) {
                entry.workspace_disposition = Some(disposition);
                entry.ready = false;
            }
        }
        Ok(())
    }

    pub(super) async fn close_published_resources(
        &self,
        id: &str,
        incarnation: &Arc<()>,
    ) -> Result<(), ThreadAssemblyError> {
        let cleanup = {
            let state = self.0.state();
            if state.unpublished_children.contains_key(id) {
                return Ok(());
            }
            state
                .entries
                .get(id)
                .filter(|entry| {
                    entry.parent_id.is_some() && Arc::ptr_eq(&entry.incarnation, incarnation)
                })
                .map(|entry| entry.cleanup.clone())
        };
        let Some(cleanup) = cleanup else {
            return Ok(());
        };
        let _permit = cleanup.acquire().await;
        let resource = {
            let state = self.0.state();
            state
                .entries
                .get(id)
                .filter(|entry| {
                    !entry.published_resources_closed
                        && Arc::ptr_eq(&entry.incarnation, incarnation)
                })
                .and_then(|entry| {
                    state
                        .child_factory
                        .clone()
                        .map(|factory| (factory, entry.workspace_disposition.unwrap_or_default()))
                })
        };
        if let Some((factory, disposition)) = resource {
            factory.close_published(id, disposition).await?;
            if let Some(entry) = self.0.state().entries.get_mut(id)
                && Arc::ptr_eq(&entry.incarnation, incarnation)
            {
                entry.published_resources_closed = true;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::{ChildThreadRequest, StudioChildFactory, StudioThreadSpec, tests::spec};
    use super::*;
    use pretty_assertions::assert_eq;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[derive(Debug, Clone)]
    struct Resources {
        fail: Arc<AtomicBool>,
        calls: Arc<AtomicUsize>,
    }
    impl StudioChildFactory for Resources {
        async fn prepare(
            &self,
            _: ChildThreadRequest,
        ) -> Result<StudioThreadSpec, ThreadAssemblyError> {
            Err(ThreadAssemblyError::Closed)
        }
        async fn discard_unpublished(&self, _: &str) -> Result<(), ThreadAssemblyError> {
            Ok(())
        }
        async fn close_published(
            &self,
            _: &str,
            disposition: AgentWorkspaceDisposition,
        ) -> Result<(), ThreadAssemblyError> {
            assert_eq!(disposition, AgentWorkspaceDisposition::Cleanup);
            self.calls.fetch_add(1, Ordering::SeqCst);
            tokio::task::yield_now().await;
            if self.fail.load(Ordering::SeqCst) {
                Err(ThreadAssemblyError::Closed)
            } else {
                Ok(())
            }
        }
    }

    #[derive(Debug, Clone)]
    struct FailingDurability {
        fail: Arc<AtomicBool>,
        durable: Arc<std::sync::atomic::AtomicU64>,
    }
    impl pl_core::thread::cold::ColdStore for FailingDurability {
        fn admit(
            &self,
            _: &str,
            _: u64,
            _: pl_core::context::OpaquePayload,
        ) -> Result<(), pl_core::thread::cold::ColdStoreError> {
            Ok(())
        }
        async fn flush(
            &self,
            _: &str,
            sequence: u64,
        ) -> Result<(), pl_core::thread::cold::ColdStoreError> {
            if self.fail.load(Ordering::SeqCst) {
                return Err(pl_core::thread::cold::ColdStoreError {
                    source: Box::new(std::io::Error::other("durability unavailable")),
                });
            }
            self.durable.store(sequence, Ordering::SeqCst);
            Ok(())
        }
    }

    #[tokio::test]
    async fn closed_lifecycle_with_failed_durability_retains_workspace_until_successful_retry() {
        let directory = tempfile::tempdir().unwrap();
        let owner = StudioThreadAssembler::default();
        let resources = Resources {
            fail: Arc::new(AtomicBool::new(false)),
            calls: Arc::new(AtomicUsize::new(0)),
        };
        owner.set_child_factory(resources.clone()).unwrap();
        owner
            .assemble(spec("root", None, directory.path()))
            .await
            .unwrap();
        let durability = FailingDurability {
            fail: Arc::new(AtomicBool::new(false)),
            durable: Default::default(),
        };
        let mut child_spec = spec("child", Some("root"), directory.path());
        child_spec.cold_store = Some(pl_core::thread::cold::ColdStoreHandle::new(
            durability.clone(),
        ));
        let child = owner.assemble(child_spec).await.unwrap();
        owner
            .seal_agent_close(&[(1, "child".into())], AgentWorkspaceDisposition::Cleanup)
            .unwrap();
        durability.fail.store(true, Ordering::SeqCst);
        let error = owner.close("child").await.unwrap_err();
        let ThreadAssemblyError::Thread(pl_core::thread::ThreadError::Storage(source)) = error
        else {
            panic!("durable close failure must preserve its typed storage source: {error:?}");
        };
        assert!(source.to_string().contains("durability unavailable"));
        let closed = child.snapshot();
        assert_eq!(closed.lifecycle, pl_core::thread::ThreadLifecycle::Closing);
        let canonical = pl_core::thread::journal::replay(&child.journal().await.unwrap()).unwrap();
        assert_eq!(
            canonical.lifecycle,
            pl_core::thread::ThreadLifecycle::Closed
        );
        assert!(closed.persistence.durable_sequence < closed.commit_sequence);
        assert!(owner.0.state().entries.contains_key("child"));
        assert_eq!(resources.calls.load(Ordering::SeqCst), 0);
        assert!(
            child.close_if_idle().await.is_err(),
            "Closed without durability must retry the real barrier"
        );
        assert!(owner.close("root").await.is_err());
        durability.fail.store(false, Ordering::SeqCst);
        owner.close("child").await.unwrap();
        owner.close("child").await.unwrap();
        assert!(!owner.0.state().entries.contains_key("child"));
        assert_eq!(resources.calls.load(Ordering::SeqCst), 1);
        let closed = child.snapshot();
        assert_eq!(
            durability.durable.load(Ordering::SeqCst),
            closed.commit_sequence
        );
        assert_eq!(closed.persistence.durable_sequence, closed.commit_sequence);
        assert!(child.close_if_idle().await.unwrap());
        owner.close("root").await.unwrap();
    }

    #[tokio::test]
    async fn failed_published_cleanup_retains_owner_policy_and_parent_until_serial_retry_succeeds()
    {
        let directory = tempfile::tempdir().unwrap();
        let owner = StudioThreadAssembler::default();
        let resources = Resources {
            fail: Arc::new(AtomicBool::new(true)),
            calls: Arc::new(AtomicUsize::new(0)),
        };
        owner.set_child_factory(resources.clone()).unwrap();
        owner
            .assemble(spec("root", None, directory.path()))
            .await
            .unwrap();
        let child = owner
            .assemble(spec("child", Some("root"), directory.path()))
            .await
            .unwrap();
        let targets = vec![(1, "child".into())];
        owner
            .seal_agent_close(&targets, AgentWorkspaceDisposition::Cleanup)
            .unwrap();
        assert!(owner.close("child").await.is_err());
        assert_eq!(
            child.snapshot().lifecycle,
            pl_core::thread::ThreadLifecycle::Closed
        );
        assert!(owner.thread("child").is_none());
        assert!(owner.0.state().entries.contains_key("child"));
        assert!(matches!(
            owner.close("root").await,
            Err(ThreadAssemblyError::Descendant(_))
        ));
        assert!(matches!(
            owner.seal_agent_close(&targets, AgentWorkspaceDisposition::Preserve),
            Err(ThreadAssemblyError::CloseDisposition(_))
        ));
        let calls = resources.calls.load(Ordering::SeqCst);
        resources.fail.store(false, Ordering::SeqCst);
        let (first, second) = tokio::join!(owner.close("child"), owner.close("child"));
        first.unwrap();
        second.unwrap();
        assert_eq!(resources.calls.load(Ordering::SeqCst), calls + 1);
        assert!(!owner.0.state().entries.contains_key("child"));
        owner.close("root").await.unwrap();
    }
}
