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
