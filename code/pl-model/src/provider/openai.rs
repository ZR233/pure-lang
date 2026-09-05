//! Native OpenAi access. Completion uses the same lifecycle and accounting as routed calls.
use crate::completion::{CompletionFailure, CompletionRequest, CompletionResponse};
use crate::runtime::{InvocationRunner, ModelInvocationContext};

/// Concrete OpenAi client, obtained from the resolved runtime's provider enum.
#[derive(Debug, Clone)]
pub struct OpenAiClient {
    pub(crate) runner: InvocationRunner,
}

/// OpenAI cache placement mode.
#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheMode {
    Implicit,
    Explicit,
}

/// OpenAI cache controls, used only by the native OpenAI client.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PromptCacheOptions {
    pub mode: CacheMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl: Option<String>,
}

/// Native Responses settings. Tool definitions remain in the common request.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct OpenAiCompletionOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_options: Option<PromptCacheOptions>,
}

/// Common completion input combined with typed native settings.
#[derive(Debug, Clone)]
pub struct OpenAiCompletion {
    pub request: CompletionRequest,
    pub options: OpenAiCompletionOptions,
}

impl OpenAiClient {
    /// Executes a native request with shared cancellation, retries and final accounting.
    ///
    /// # Errors
    /// Returns a typed failure retaining any usage reported before the failure.
    pub async fn complete(
        &self,
        input: OpenAiCompletion,
        context: ModelInvocationContext,
    ) -> Result<CompletionResponse, CompletionFailure> {
        let body = super::clients::native_body(input.options)?;
        self.runner
            .with_native_body(body)
            .complete(input.request, context)
            .await
    }
}
