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
    pub prepared_content: Vec<crate::completion::PreparedContentPart>,
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
        completion.prepared_content = request.prepared_content;
        completion.reasoning = request.reasoning;
        let response = self
            .complete(completion, invocation.with_session(session.clone()))
            .await;
        let cleanup = session.close().await;
        let response = match (response, cleanup) {
            (Ok(response), Ok(())) => response,
            (Ok(response), Err(source)) => {
                return Err(CompletionFailure {
                    source,
                    accounting: Box::new(response.accounting),
                });
            }
            (Err(failure), Ok(())) => return Err(failure),
            (Err(failure), Err(cleanup)) => {
                return Err(CompletionFailure {
                    source: PureError::Io(std::io::Error::other(SummaryCleanupFailure {
                        primary: failure.source,
                        cleanup,
                    })),
                    accounting: failure.accounting,
                });
            }
        };
        let Some(text) = response.content.filter(|text| !text.trim().is_empty()) else {
            return Err(CompletionFailure {
                source: PureError::LlmError(request.empty_summary_error.to_owned()),
                accounting: Box::new(response.accounting),
            });
        };
        Ok(TextSummary {
            text,
            accounting: response.accounting,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ModelInfo;
    use crate::provider::ProviderEndpoint;
    use crate::runtime::test_support::serve_sse_checked;
    use pretty_assertions::assert_eq;

    #[tokio::test]
    async fn summary_uses_its_own_session_and_disables_tool_selection() {
        let events = [
            serde_json::json!({"choices":[{"delta":{"content":"  exact summary\n"},"finish_reason":null}]}),
            serde_json::json!({"choices":[{"delta":{},"finish_reason":"stop"}]}),
            serde_json::json!({"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":4,"total_tokens":14}}),
        ];
        let sse = events
            .into_iter()
            .map(|event| format!("data: {event}\n\n"))
            .collect::<String>()
            + "data: [DONE]\n\n";
        let (url, server) = serve_sse_checked(sse, |request| {
            request.body["tool_choice"] == "none"
                && request.body["tools"]
                    .as_array()
                    .is_some_and(|tools| tools.len() == 1)
        })
        .await;
        let runtime = ModelRuntime::new(
            ProviderEndpoint::deepseek(Some(url)),
            ModelInfo::compatible("summary-test"),
        )
        .unwrap();
        let main_session = ModelSession::default();
        main_session.close().await.unwrap();
        let tools = [
            ToolSpec::function("local", "local", serde_json::json!({"type":"object"})),
            ToolSpec::ProgrammaticToolCalling,
        ];
        let summary = runtime
            .summarize(
                TextSummaryRequest {
                    instructions: "stable instructions",
                    input: Vec::new(),
                    prepared_content: Vec::new(),
                    reasoning: None,
                    tools: &tools,
                    requirement: "summarize",
                    max_output_tokens: Some(128),
                    empty_summary_error: "empty summary",
                },
                ModelInvocationContext::new(main_session),
            )
            .await
            .unwrap();
        server.await.unwrap();
        assert_eq!(summary.text, "  exact summary\n");
    }
}
