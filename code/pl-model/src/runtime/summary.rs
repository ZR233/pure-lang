//! Text summarization with frozen history and an independent physical session.
use super::{ModelInvocationContext, ModelRuntime, ModelSession};
use crate::completion::{
    CompletionFailure, InferenceAccounting, ModelContextItem, PureError, ToolSpec, summary_request,
};

/// Supplied model-visible prefix and the host's summary requirement.
#[derive(Debug)]
pub struct TextSummaryRequest<'a> {
    pub instructions: &'a str,
    pub input: Vec<ModelContextItem>,
    pub attachments: Vec<crate::completion::AttachmentInput>,
    pub reasoning: Option<crate::completion::ReasoningConfig>,
    pub tools: &'a [ToolSpec],
    pub requirement: &'a str,
    pub max_output_tokens: Option<u64>,
    pub empty_summary_error: &'a str,
}

/// Actual summary text and service accounting, before any history replacement.
#[derive(Debug)]
pub struct TextSummary {
    pub text: String,
    pub accounting: InferenceAccounting,
    pub model_observation: Option<crate::completion::InferenceModelObservation>,
}

#[derive(Debug, thiserror::Error)]
#[error("summary failed ({primary}); its private session also failed to close ({cleanup})")]
struct SummaryCleanupFailure {
    #[source]
    primary: PureError,
    cleanup: PureError,
}

impl ModelRuntime {
    /// Summarizes the exact supplied prefix without advancing the caller's model session.
    /// Hosted capabilities are removed and local tool selection is disabled.
    ///
    /// # Errors
    /// Preserves observed usage on transport, empty-summary or private-session close failure.
    pub async fn summarize(
        &self,
        request: TextSummaryRequest<'_>,
        invocation: ModelInvocationContext,
    ) -> Result<TextSummary, CompletionFailure> {
        let session = ModelSession::default();
        let mut completion = summary_request(
            request.instructions,
            request.input,
            request.tools,
            request.requirement,
            request.max_output_tokens,
        );
        completion.attachments = request.attachments;
        completion.reasoning = request.reasoning;
        let response = self
            .complete(completion, invocation.with_session(session.clone()))
            .await;
        let cleanup = session.close().await;
        let response = match (response, cleanup) {
            (Ok(response), Ok(())) => response,
            (Ok(response), Err(source)) => {
                return Err(CompletionFailure {
                    source: Box::new(source),
                    accounting: Box::new(response.accounting),
                    model_observation: response.model_observation.map(Box::new),
                    presentation_items: response.presentation_items,
                    cancelled: false,
                });
            }
            (Err(failure), Ok(())) => return Err(failure),
            (Err(failure), Err(cleanup)) => {
                // Folding a separate cleanup failure in does not erase the primary call's fact.
                let cancelled = failure.is_cancelled();
                return Err(CompletionFailure {
                    source: Box::new(PureError::Io(std::io::Error::other(
                        SummaryCleanupFailure {
                            primary: *failure.source,
                            cleanup,
                        },
                    ))),
                    accounting: failure.accounting,
                    model_observation: failure.model_observation,
                    presentation_items: failure.presentation_items,
                    cancelled,
                });
            }
        };
        let model_observation = response.model_observation.clone();
        let Some(text) = response.content.filter(|text| !text.trim().is_empty()) else {
            return Err(CompletionFailure {
                source: Box::new(PureError::LlmError(request.empty_summary_error.to_owned())),
                accounting: Box::new(response.accounting),
                model_observation: model_observation.clone().map(Box::new),
                presentation_items: response.presentation_items,
                cancelled: false,
            });
        };
        Ok(TextSummary {
            text,
            accounting: response.accounting,
            model_observation,
        })
    }
}
