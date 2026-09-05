//! Native DeepSeek access. Completion uses the same lifecycle and accounting as routed calls.
use crate::completion::{CompletionFailure, CompletionRequest, CompletionResponse};
use crate::runtime::{InvocationRunner, ModelInvocationContext};

/// Concrete DeepSeek client, obtained from the resolved runtime's provider enum.
#[derive(Debug, Clone)]
pub struct DeepSeekClient {
    pub(crate) runner: InvocationRunner,
}

/// DeepSeek thinking mode.
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DeepSeekThinking {
    #[default]
    Enabled,
    Disabled,
}

/// DeepSeek-specific inference controls.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct DeepSeekCompletionOptions {
    pub thinking: DeepSeekThinking,
}

/// Common completion input combined with typed native settings.
#[derive(Debug, Clone)]
pub struct DeepSeekCompletion {
    pub request: CompletionRequest,
    pub options: DeepSeekCompletionOptions,
}

impl DeepSeekClient {
    /// Executes a native request with shared cancellation, retries and final accounting.
    ///
    /// # Errors
    /// Returns a typed failure retaining any usage reported before the failure.
    pub async fn complete(
        &self,
        input: DeepSeekCompletion,
        context: ModelInvocationContext,
    ) -> Result<CompletionResponse, CompletionFailure> {
        let body = super::clients::native_body(input.options)?;
        self.runner
            .with_native_body(body)
            .complete(input.request, context)
            .await
    }
}
