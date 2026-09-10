//! Model invocation ports. Implementations own protocols, encoding, and physical connections.

use std::fmt;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
pub use tokio_util::sync::CancellationToken;

use crate::context::{ContextContent, ContextSnapshot, OpaquePayload};

/// Factory for independent Thread sessions. Clients may share pools, but never mutable history.
pub trait Model: Send + Sync + 'static {
    /// Opens a fresh session; the returned owner must be closed before its Thread is released.
    fn open_session(&self) -> impl Future<Output = Result<DynModelSession, ModelError>> + Send;
}

trait ErasedModel: Send + Sync {
    fn open_session(&self) -> BoxFuture<'_, Result<DynModelSession, ModelError>>;
}
impl<T: Model> ErasedModel for T {
    fn open_session(&self) -> BoxFuture<'_, Result<DynModelSession, ModelError>> {
        Box::pin(Model::open_session(self))
    }
}

/// Shareable factory erasure; each invocation must return an independently owned session.
#[derive(Clone)]
pub struct ModelFactory(Arc<dyn ErasedModel>);
impl ModelFactory {
    /// Erases a model factory without opening a session or starting model work.
    pub fn new(model: impl Model) -> Self {
        Self(Arc::new(model))
    }
    /// Opens a session with panic isolation at the implementation boundary.
    ///
    /// # Errors
    /// Preserves model construction failures and reports panicking factories.
    pub async fn open_session(&self) -> Result<DynModelSession, ModelError> {
        crate::error_record::catch_boundary("model factory", async { self.0.open_session().await })
            .await
            .map_err(ModelError::from_panic)?
    }
}
impl fmt::Debug for ModelFactory {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ModelFactory")
            .finish_non_exhaustive()
    }
}

/// Serial model resource owned by one Thread incarnation.
pub trait ModelSession: Send + 'static {
    /// Freezes implementation selection and request material before dispatch.
    fn prepare(
        &mut self,
        request: ModelRequest,
    ) -> impl Future<Output = Result<PreparedModelCall, ModelError>> + Send;

    /// Stops and waits for owned resources. A failed close retains the implementation for retry.
    fn close(&mut self) -> impl Future<Output = Result<(), ModelError>> + Send;
}

/// Declaration sent alongside context. Core does not interpret its producer-owned schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelToolDeclaration {
    pub tool_id: String,
    pub declaration: OpaquePayload,
}

/// Maximum tool-call concurrency admitted by the frozen executor catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCallMode {
    Sequential,
    Parallel,
}

/// Immutable preparation input. Cancellation remains live while the content stays frozen.
#[derive(Debug, Clone)]
pub struct ModelRequest {
    /// Optional coalescing live observation; never commits canonical content.
    pub progress: Option<ModelProgressSender>,
    pub thread_id: String,
    pub turn_id: String,
    pub attempt_id: String,
    pub context: ContextSnapshot,
    pub tools: Arc<[ModelToolDeclaration]>,
    pub tool_call_mode: ToolCallMode,
    /// Frozen tool IDs that must be the only tool call in a response.
    pub solo_tool_ids: Arc<[String]>,
    pub committed_private_context: Option<OpaquePayload>,
    pub resources: Option<crate::context::ResourceAccess>,
    pub cancellation: CancellationToken,
}

/// A complete live preview of the current request, separate from committed model output.
#[derive(Debug, Clone, Default)]
pub struct ModelProgress {
    pub content: Vec<ContextContent>,
    pub reasoning: Option<OpaquePayload>,
}

/// A bounded latest-value observer. Slow observers coalesce previews without blocking execution.
#[derive(Debug, Clone)]
pub struct ModelProgressSender(tokio::sync::watch::Sender<ModelProgress>);
impl ModelProgressSender {
    pub(crate) fn channel() -> (Self, tokio::sync::watch::Receiver<ModelProgress>) {
        let (sender, receiver) = tokio::sync::watch::channel(ModelProgress::default());
        (Self(sender), receiver)
    }
    /// Publishes a complete preview. It cannot advance a Thread context or grant execution authority.
    pub fn publish(&self, progress: ModelProgress) {
        self.0.send_replace(progress);
    }
}

/// Current request identity supplied by the framework with its last observed preview.
#[derive(Debug, Clone)]
pub struct ActiveModelProgress {
    pub attempt_id: String,
    pub progress: ModelProgress,
}

/// An executable local call normalized by the model implementation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelToolCall {
    pub call_id: String,
    pub tool_id: String,
    pub arguments: OpaquePayload,
}

/// A proposed step. The Thread commits its content and private context together.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStepOutput {
    pub attempt_id: String,
    pub base_context_revision: u64,
    pub content: Vec<ContextContent>,
    pub tool_calls: Vec<ModelToolCall>,
    pub private_context: Option<OpaquePayload>,
    pub usage: ModelUsage,
}

/// Service-reported counters. Cache values are subsets of total input, not additional input.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelUsage {
    pub input_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
}

/// Portable failure classes. Provider-specific details are retained as the error source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelFailureKind {
    Cancelled,
    UnsupportedContent,
    IncompatibleContext,
    ContextLimit,
    Unavailable,
    InvalidResponse,
    ImplementationPanicked,
}

/// Invocation failure with usage observed before termination.
#[derive(Debug, thiserror::Error, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[error("model call failed: {kind:?}")]
pub struct ModelError {
    /// Producer-owned facts observed on failure, including optional accounting or protocol material.
    #[serde(default)]
    pub details: Option<Box<OpaquePayload>>,
    pub kind: ModelFailureKind,
    pub usage: ModelUsage,
    #[source]
    #[serde(with = "crate::error_record::optional")]
    pub source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl ModelError {
    fn from_panic(source: crate::error_record::BoundaryPanic) -> Self {
        Self {
            kind: ModelFailureKind::ImplementationPanicked,
            details: None,
            usage: ModelUsage::default(),
            source: Some(Box::new(source)),
        }
    }
}

/// Accuracy declared by the model adapter's token estimator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EstimateAccuracy {
    Exact,
    Approximate,
}

/// Model-supplied input token estimate. Absence remains unknown, not zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenEstimate {
    pub tokens: u64,
    pub accuracy: EstimateAccuracy,
}

#[derive(Default)]
struct SessionHealth {
    poisoned: AtomicBool,
    closing: AtomicBool,
}
impl SessionHealth {
    fn ensure_available(&self) -> Result<(), ModelError> {
        if self.poisoned.load(Ordering::Acquire) || self.closing.load(Ordering::Acquire) {
            Err(ModelError {
                details: None,
                kind: ModelFailureKind::Unavailable,
                usage: ModelUsage::default(),
                source: Some(Box::new(std::io::Error::other(
                    "model session requires replacement or is closing",
                ))),
            })
        } else {
            Ok(())
        }
    }
}

/// One-shot invocation bound to the implementation that prepared its request.
///
/// Preparation cannot start unowned model work. Executing consumes this handle; a second
/// dispatch requires a separately identified attempt. Dropping an unstarted call drops its future.
pub struct PreparedModelCall {
    request_metadata: Option<OpaquePayload>,
    tool_projection: Option<OpaquePayload>,
    session_health: Option<Arc<SessionHealth>>,
    input_estimate: Option<TokenEstimate>,
    future: BoxFuture<'static, Result<ModelStepOutput, ModelError>>,
}

impl PreparedModelCall {
    /// Wraps a prepared, cancellation-aware operation without polling it.
    pub fn new(
        future: impl Future<Output = Result<ModelStepOutput, ModelError>> + Send + 'static,
    ) -> Self {
        Self {
            future: Box::pin(future),
            input_estimate: None,
            session_health: None,
            tool_projection: None,
            request_metadata: None,
        }
    }

    /// Freezes adapter-owned request provenance without exposing provider types to core.
    pub fn with_request_metadata(mut self, metadata: OpaquePayload) -> Self {
        self.request_metadata = Some(metadata);
        self
    }

    /// Returns the metadata of this exact prepared request.
    pub fn request_metadata(&self) -> Option<&OpaquePayload> {
        self.request_metadata.as_ref()
    }

    /// Freezes model-owned materials for tools produced by this invocation. Core does not decode them.
    pub fn with_tool_projection(mut self, projection: OpaquePayload) -> Self {
        self.tool_projection = Some(projection);
        self
    }

    /// Returns the exact tool projection materials associated with this prepared adapter.
    pub fn tool_projection(&self) -> Option<&OpaquePayload> {
        self.tool_projection.as_ref()
    }

    /// Attaches the estimate for this exact prepared input, not a separately reconstructed request.
    pub fn with_input_estimate(mut self, estimate: TokenEstimate) -> Self {
        self.input_estimate = Some(estimate);
        self
    }

    /// Returns the adapter's estimate and declared precision for the frozen input.
    pub fn input_estimate(&self) -> Option<TokenEstimate> {
        self.input_estimate
    }

    /// Dispatches this attempt exactly once.
    ///
    /// # Errors
    /// Returns the implementation failure with any observed usage.
    pub async fn execute(self) -> Result<ModelStepOutput, ModelError> {
        if let Some(health) = &self.session_health {
            health.ensure_available()?;
        }
        match crate::error_record::catch_boundary("model execution", self.future).await {
            Ok(result) => result,
            Err(source) => {
                if let Some(health) = self.session_health {
                    health.poisoned.store(true, Ordering::Release);
                }
                Err(ModelError::from_panic(source))
            }
        }
    }
}

impl fmt::Debug for PreparedModelCall {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedModelCall")
            .finish_non_exhaustive()
    }
}

trait ErasedModelSession: Send {
    fn prepare(
        &mut self,
        request: ModelRequest,
    ) -> BoxFuture<'_, Result<PreparedModelCall, ModelError>>;
    fn close(&mut self) -> BoxFuture<'_, Result<(), ModelError>>;
}

impl<T: ModelSession> ErasedModelSession for T {
    fn prepare(
        &mut self,
        request: ModelRequest,
    ) -> BoxFuture<'_, Result<PreparedModelCall, ModelError>> {
        Box::pin(ModelSession::prepare(self, request))
    }

    fn close(&mut self) -> BoxFuture<'_, Result<(), ModelError>> {
        Box::pin(ModelSession::close(self))
    }
}

/// Owned erasure for one serial model session, deliberately not Clone or Sync.
pub struct DynModelSession {
    health: Arc<SessionHealth>,
    inner: Box<dyn ErasedModelSession>,
}

impl DynModelSession {
    /// Takes ownership of a Thread-local implementation.
    pub fn new(session: impl ModelSession) -> Self {
        Self {
            inner: Box::new(session),
            health: Arc::new(SessionHealth::default()),
        }
    }

    pub(crate) fn is_available(&self) -> bool {
        self.health.ensure_available().is_ok()
    }

    /// Prepares the next immutable attempt.
    ///
    /// # Errors
    /// Returns invalid input, incompatible context, or preparation failure.
    pub async fn prepare(
        &mut self,
        request: ModelRequest,
    ) -> Result<PreparedModelCall, ModelError> {
        self.health.ensure_available()?;
        match crate::error_record::catch_boundary("model preparation", async {
            self.inner.prepare(request).await
        })
        .await
        {
            Ok(result) => result.map(|mut call| {
                call.session_health = Some(self.health.clone());
                call
            }),
            Err(source) => {
                self.health.poisoned.store(true, Ordering::Release);
                Err(ModelError::from_panic(source))
            }
        }
    }

    /// Closes resources before the Thread releases its model session.
    ///
    /// # Errors
    /// A failed close retains this owner so the caller can retry.
    pub async fn close(&mut self) -> Result<(), ModelError> {
        self.health.closing.store(true, Ordering::Release);
        crate::error_record::catch_boundary("model close", async { self.inner.close().await })
            .await
            .map_err(ModelError::from_panic)?
    }
}

impl fmt::Debug for DynModelSession {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DynModelSession")
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct PanickingSession;
    impl ModelSession for PanickingSession {
        async fn prepare(&mut self, _: ModelRequest) -> Result<PreparedModelCall, ModelError> {
            Ok(PreparedModelCall::new(async {
                panic!("broken model implementation")
            }))
        }
        async fn close(&mut self) -> Result<(), ModelError> {
            Ok(())
        }
    }
    fn request() -> ModelRequest {
        ModelRequest {
            tool_call_mode: crate::model::ToolCallMode::Parallel,
            solo_tool_ids: Vec::new().into(),
            progress: None,
            thread_id: "thread".into(),
            turn_id: "turn".into(),
            attempt_id: "attempt".into(),
            context: Default::default(),
            tools: Arc::from([]),
            committed_private_context: None,
            resources: None,
            cancellation: CancellationToken::new(),
        }
    }
    #[tokio::test]
    async fn implementation_panic_invalidates_the_session_and_prepared_siblings_but_allows_close() {
        let mut model = DynModelSession::new(PanickingSession);
        let first = model.prepare(request()).await.unwrap();
        let sibling = model.prepare(request()).await.unwrap();
        assert_eq!(
            first.execute().await.unwrap_err().kind,
            ModelFailureKind::ImplementationPanicked
        );
        assert_eq!(
            sibling.execute().await.unwrap_err().kind,
            ModelFailureKind::Unavailable
        );
        assert_eq!(
            model.prepare(request()).await.unwrap_err().kind,
            ModelFailureKind::Unavailable
        );
        model.close().await.unwrap();
    }
}
