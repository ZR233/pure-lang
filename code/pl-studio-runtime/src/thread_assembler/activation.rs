//! Owned, coalesced preparation before publishing a root or restored Thread.
use super::*;
use futures::FutureExt;
use std::future::Future;

/// Product identity selected before allocating physical resources.
#[derive(Debug, Clone)]
pub struct ThreadActivation {
    pub id: String,
    pub parent_id: Option<String>,
}

/// Preparation cancellation belongs to the assembler, independently of individual waiting clients.
#[derive(Debug, Clone)]
pub struct ThreadPreparation {
    pub identity: ThreadActivation,
    pub cancellation: tokio_util::sync::CancellationToken,
}

/// Resolves an existing product task into fresh model/tool resources and saved facts.
pub trait StudioActivationFactory: std::fmt::Debug + Send + Sync + 'static {
    /// Owns cleanup of allocations when preparation fails before returning a spec.
    /// Must honor cancellation and must not execute restored model calls or tool effects.
    fn prepare(
        &self,
        request: ThreadPreparation,
    ) -> impl Future<Output = Result<StudioThreadSpec, ThreadAssemblyError>> + Send;
}

impl StudioThreadAssembler {
    /// Coalesces concurrent activation and owns preparation even if a waiting client disconnects.
    ///
    /// # Errors
    /// Rejects mismatched parent identities, closing owners, failed resources and corrupt history.
    pub async fn activate(
        &self,
        identity: ThreadActivation,
        factory: impl StudioActivationFactory,
    ) -> Result<ThreadHandle, ThreadAssemblyError> {
        loop {
            let changed = self.0.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let waiting = {
                let state = self.0.state();
                if state.closing {
                    return Err(ThreadAssemblyError::Closed);
                }
                if let Some(entry) = state.entries.get(&identity.id) {
                    if entry.parent_id != identity.parent_id {
                        return Err(ThreadAssemblyError::Identity(identity.id));
                    }
                    if entry.ready && entry.thread.snapshot().lifecycle == ThreadLifecycle::Open {
                        return Ok(entry.thread.clone());
                    }
                    if !state.creating.contains_key(&identity.id) {
                        return Err(ThreadAssemblyError::Unpublished(identity.id));
                    }
                }
                match state.creating.get(&identity.id) {
                    Some(preparation) if preparation.parent_id == identity.parent_id => {
                        Some(preparation.result.clone())
                    }
                    Some(_) => return Err(ThreadAssemblyError::Identity(identity.id)),
                    None => None,
                }
            };
            match waiting {
                Some(Some(result)) => return await_result(result).await,
                Some(None) => {
                    changed.await;
                    continue;
                }
                None => {}
            }
            let reservation = match self.reserve(&identity.id, identity.parent_id.as_deref()) {
                Ok(reservation) => reservation,
                Err(ThreadAssemblyError::Identity(_))
                    if {
                        let state = self.0.state();
                        state.creating.contains_key(&identity.id)
                            || state.entries.contains_key(&identity.id)
                    } =>
                {
                    continue;
                }
                Err(error) => return Err(error),
            };
            let (publication, result) = tokio::sync::watch::channel(None);
            let cancellation = {
                let mut state = self.0.state();
                let preparation = state
                    .creating
                    .get_mut(&identity.id)
                    .ok_or(ThreadAssemblyError::Closed)?;
                preparation.result = Some(result.clone());
                preparation.cancellation.clone()
            };
            self.0.changed.notify_waiters();
            let owner = self.clone();
            tokio::spawn(async move {
                let operation = std::panic::AssertUnwindSafe(async {
                    let spec = factory
                        .prepare(ThreadPreparation {
                            identity: identity.clone(),
                            cancellation: cancellation.clone(),
                        })
                        .await?;
                    if spec.id != identity.id || spec.parent_id != identity.parent_id {
                        return Err(ThreadAssemblyError::Identity(spec.id));
                    }
                    if cancellation.is_cancelled() {
                        return Err(ThreadAssemblyError::Closed);
                    }
                    owner.assemble_reserved(spec, &reservation).await
                })
                .catch_unwind()
                .await;
                let outcome = operation.unwrap_or(Err(ThreadAssemblyError::ActivationPanicked));
                publication.send_replace(Some(outcome.map_err(Arc::new)));
                drop(reservation);
            });
            return await_result(result).await;
        }
    }
}

async fn await_result(
    mut result: tokio::sync::watch::Receiver<Option<ActivationOutcome>>,
) -> Result<ThreadHandle, ThreadAssemblyError> {
    loop {
        if let Some(outcome) = result.borrow_and_update().clone() {
            return outcome.map_err(ThreadAssemblyError::ActivationFailed);
        }
        result
            .changed()
            .await
            .map_err(|_| ThreadAssemblyError::Closed)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug, Clone)]
    struct PausedFactory {
        directory: std::path::PathBuf,
        calls: Arc<AtomicUsize>,
        started: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
        cancelled: Arc<tokio::sync::Notify>,
        cleanup: Arc<tokio::sync::Notify>,
    }
    impl StudioActivationFactory for PausedFactory {
        async fn prepare(
            &self,
            request: ThreadPreparation,
        ) -> Result<StudioThreadSpec, ThreadAssemblyError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.started.notify_one();
            tokio::select! {
                _ = self.release.notified() => Ok(super::super::tests::spec(&request.identity.id, request.identity.parent_id.as_deref(), &self.directory)),
                _ = request.cancellation.cancelled() => {
                    self.cancelled.notify_one();
                    self.cleanup.notified().await;
                    Err(ThreadAssemblyError::Closed)
                }
            }
        }
    }
    fn factory(directory: &std::path::Path) -> PausedFactory {
        PausedFactory {
            directory: directory.into(),
            calls: Default::default(),
            started: Default::default(),
            release: Default::default(),
            cancelled: Default::default(),
            cleanup: Default::default(),
        }
    }
    fn identity() -> ThreadActivation {
        ThreadActivation {
            id: "root".into(),
            parent_id: None,
        }
    }

    #[tokio::test]
    async fn disconnecting_one_waiter_keeps_shared_activation_owned_for_other_clients() {
        let directory = tempfile::tempdir().unwrap();
        let owner = StudioThreadAssembler::default();
        let factory = factory(directory.path());
        let first = tokio::spawn({
            let owner = owner.clone();
            let factory = factory.clone();
            async move { owner.activate(identity(), factory).await }
        });
        factory.started.notified().await;
        first.abort();
        let _ = first.await;
        let second = tokio::spawn({
            let owner = owner.clone();
            let factory = factory.clone();
            async move { owner.activate(identity(), factory).await }
        });
        factory.release.notify_one();
        let thread = tokio::time::timeout(std::time::Duration::from_secs(2), second)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(factory.calls.load(Ordering::SeqCst), 1);
        assert!(owner.thread("root").is_some());
        assert!(owner.close_all().await.is_empty());
        assert_eq!(thread.snapshot().lifecycle, ThreadLifecycle::Closed);
    }

    #[tokio::test]
    async fn shutdown_cancels_and_waits_for_unpublished_activation_cleanup() {
        let directory = tempfile::tempdir().unwrap();
        let owner = StudioThreadAssembler::default();
        let factory = factory(directory.path());
        let activation = tokio::spawn({
            let owner = owner.clone();
            let factory = factory.clone();
            async move { owner.activate(identity(), factory).await }
        });
        factory.started.notified().await;
        let closing = tokio::spawn({
            let owner = owner.clone();
            async move { owner.close_all().await }
        });
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            factory.cancelled.notified(),
        )
        .await
        .unwrap();
        assert!(
            !closing.is_finished(),
            "preparation still owns its resource cleanup"
        );
        factory.cleanup.notify_one();
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(2), closing)
                .await
                .unwrap()
                .unwrap()
                .is_empty()
        );
        assert!(activation.await.unwrap().is_err());
        assert!(owner.0.state().creating.is_empty());
    }
}
