//! Basic OpenAI-compatible access without vendor capability inheritance.
use crate::completion::{CompletionFailure, CompletionRequest, CompletionResponse};
use crate::runtime::{InvocationRunner, ModelInvocationContext};

/// Standard compatible client. Optional authentication belongs to its endpoint.
#[derive(Debug, Clone)]
pub struct CompatibleClient {
    pub(crate) runner: InvocationRunner,
}

impl CompatibleClient {
    /// Runs a standard request with complete final usage collection.
    ///
    /// # Errors
    /// Returns the provider failure and any accounting already received.
    pub async fn complete(
        &self,
        request: CompletionRequest,
        context: ModelInvocationContext,
    ) -> Result<CompletionResponse, CompletionFailure> {
        self.runner.complete(request, context).await
    }
}
