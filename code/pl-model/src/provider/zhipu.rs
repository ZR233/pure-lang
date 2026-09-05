//! Native Zhipu access. Completion uses the same lifecycle and accounting as routed calls.
use crate::completion::{CompletionFailure, CompletionRequest, CompletionResponse};
use crate::runtime::{InvocationRunner, ModelInvocationContext};

/// Concrete Zhipu client, obtained from the resolved runtime's provider enum.
#[derive(Debug, Clone)]
pub struct ZhipuClient {
    pub(crate) runner: InvocationRunner,
}

/// Native tool argument streaming, independent of standard chat streaming.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ZhipuCompletionOptions {
    pub tool_stream: bool,
}
impl Default for ZhipuCompletionOptions {
    fn default() -> Self {
        Self { tool_stream: true }
    }
}

/// Common completion input combined with typed native settings.
#[derive(Debug, Clone)]
pub struct ZhipuCompletion {
    pub request: CompletionRequest,
    pub options: ZhipuCompletionOptions,
}

impl ZhipuClient {
    /// Executes a native request with shared cancellation, retries and final accounting.
    ///
    /// # Errors
    /// Returns a typed failure retaining any usage reported before the failure.
    pub async fn complete(
        &self,
        input: ZhipuCompletion,
        context: ModelInvocationContext,
    ) -> Result<CompletionResponse, CompletionFailure> {
        let body = super::clients::native_body(input.options)?;
        self.runner
            .with_native_body(body)
            .complete(input.request, context)
            .await
    }
}
