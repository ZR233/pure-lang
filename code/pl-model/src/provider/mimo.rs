//! Native MiMo access. Completion uses the same lifecycle and accounting as routed calls.
use crate::completion::{CompletionFailure, CompletionRequest, CompletionResponse};
use crate::runtime::{InvocationRunner, ModelInvocationContext};

/// Concrete MiMo client, obtained from the resolved runtime's provider enum.
#[derive(Debug, Clone)]
pub struct MiMoClient {
    pub(crate) runner: InvocationRunner,
}

/// MiMo thinking mode; generated reasoning remains in tool-call history.
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MiMoThinking {
    #[default]
    Enabled,
    Disabled,
}

/// MiMo-specific inference controls.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct MiMoCompletionOptions {
    pub thinking: MiMoThinking,
}

/// Common completion input combined with typed native settings.
#[derive(Debug, Clone)]
pub struct MiMoCompletion {
    pub request: CompletionRequest,
    pub options: MiMoCompletionOptions,
}

impl MiMoClient {
    /// Executes a native request with shared cancellation, retries and final accounting.
    ///
    /// # Errors
    /// Returns a typed failure retaining any usage reported before the failure.
    pub async fn complete(
        &self,
        input: MiMoCompletion,
        context: ModelInvocationContext,
    ) -> Result<CompletionResponse, CompletionFailure> {
        let body = super::clients::native_body(input.options)?;
        self.runner
            .with_native_body(body)
            .complete(input.request, context)
            .await
    }
}
