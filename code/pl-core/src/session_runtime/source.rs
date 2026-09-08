use std::error::Error;
use std::future::Future;

use futures::future::BoxFuture;

use super::SessionBuildContext;

/// A source failure with its original host error preserved.
#[derive(Debug, thiserror::Error)]
#[error("session event source failed during {operation}: {source}")]
pub struct SessionSourceError {
    operation: &'static str,
    #[source]
    source: Box<dyn Error + Send + Sync>,
}

impl SessionSourceError {
    /// Preserves a typed backend error at the event-source boundary.
    pub fn new(operation: &'static str, source: impl Error + Send + Sync + 'static) -> Self {
        Self {
            operation,
            source: Box::new(source),
        }
    }
}

/// An initialized subscription whose pump and resources are owned by the session.
///
/// The future must observe the context cancellation token, release subscriptions,
/// and return only after cleanup. Do not detach unowned producer tasks from it.
pub struct SessionEventSubscription {
    pub(crate) run: BoxFuture<'static, Result<(), SessionSourceError>>,
}

impl SessionEventSubscription {
    /// Takes ownership of an initialized source's event and cleanup loop.
    pub fn new(run: impl Future<Output = Result<(), SessionSourceError>> + Send + 'static) -> Self {
        Self { run: Box::pin(run) }
    }
}

impl std::fmt::Debug for SessionEventSubscription {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionEventSubscription")
            .finish_non_exhaustive()
    }
}

/// Downstream extension point for session-owned message producers.
///
/// Initialization must establish the subscription before returning. The returned
/// pump starts only after the whole session is ready; queue early external events
/// in the subscription rather than awaiting publication during initialization.
pub trait SessionEventSource: Send + Sync + 'static {
    /// Initializes a subscription without publishing a partially initialized session.
    ///
    /// # Errors
    /// Returns the backend failure after releasing resources acquired by this attempt.
    fn initialize(
        &self,
        context: SessionBuildContext,
    ) -> impl Future<Output = Result<SessionEventSubscription, SessionSourceError>> + Send;
}
