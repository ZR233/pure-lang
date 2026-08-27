use std::collections::HashMap;

use pl_model::{
    CompletionRequest, CompletionTraceContext, ModelInfo, ModelInvocationContext, ModelRuntime,
    ModelSession, ProviderEndpoint, ReasoningConfig, ReasoningSummary, default_models,
    openai_default_model_slugs,
};
use pl_protocol::{Message, MessageContent, MessageRole};
use pl_trace::{AgentEvent, TraceDelta};

const OPENAI_LIVE_ENV_KEY: &str = "API_KEY_OPENAI";
const OPENAI_LIVE_BASE_URL_ENV_KEY: &str = "API_BASE_OPENAI";
const OPENAI_LIVE_MODEL_ENV_KEY: &str = "API_MODEL_OPENAI";

#[derive(Debug, Default)]
struct TraceDeltaCounts {
    text: usize,
    thinking: usize,
}

fn live_api_key() -> Option<String> {
    match std::env::var(OPENAI_LIVE_ENV_KEY) {
        Ok(value) if !value.trim().is_empty() => Some(value),
        _ => {
            eprintln!("{OPENAI_LIVE_ENV_KEY} is not set; skipping live OpenAI API test");
            None
        }
    }
}

fn user_message(content: &str) -> Message {
    Message {
        role: MessageRole::User,
        content: MessageContent::text(content.to_string()),
        reasoning_content: None,
        tool_calls: None,
        tool_result: None,
        metadata: HashMap::new(),
    }
}

fn openai_request() -> CompletionRequest {
    CompletionRequest::builder()
        .instructions(
            "Answer briefly. Use the provider's native visible output channels when available. Reply with exactly: ok."
        )
        .messages(vec![user_message("Reply with exactly: ok")])
        .max_tokens(128)
        .reasoning(Some(ReasoningConfig {
            effort: Some("medium".to_string()),
            summary: Some(ReasoningSummary::Enabled),
        }))
        .build()
}

fn live_model(slug: &str) -> ModelInfo {
    let template_slug = openai_default_model_slugs()[0];
    let mut model = default_models()
        .into_iter()
        .find(|model| model.slug == template_slug)
        .expect("OpenAI default model catalog must contain its template model");
    model.slug = slug.to_string();
    model.display_name = slug.to_string();
    model
}

async fn collect_trace_delta_counts(
    mut event_rx: tokio::sync::broadcast::Receiver<AgentEvent>,
) -> TraceDeltaCounts {
    let mut counts = TraceDeltaCounts::default();
    loop {
        match event_rx.recv().await {
            Ok(AgentEvent::TracePartDelta { event }) => match event.delta {
                TraceDelta::Text { .. } => counts.text += 1,
                TraceDelta::Thinking { .. } | TraceDelta::ReasoningContent { .. } => {
                    counts.thinking += 1;
                }
                TraceDelta::ToolArguments { .. }
                | TraceDelta::ToolResult { .. }
                | TraceDelta::Plan { .. } => {}
            },
            Ok(_) => {}
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                panic!("live OpenAI trace event receiver lagged by {skipped} events")
            }
        }
    }
    counts
}

#[tokio::test]
async fn openai_responses_smoke() {
    let Some(api_key) = live_api_key() else {
        return;
    };

    let base_url = std::env::var(OPENAI_LIVE_BASE_URL_ENV_KEY)
        .ok()
        .filter(|value| !value.trim().is_empty());
    let mut info = ProviderEndpoint::openai(base_url);
    info.bearer_token = Some(api_key);
    let model_slug = std::env::var(OPENAI_LIVE_MODEL_ENV_KEY)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| openai_default_model_slugs()[0].to_string());
    let runtime = ModelRuntime::new(info, live_model(&model_slug)).unwrap();
    let (event_tx, event_rx) = tokio::sync::broadcast::channel(256);
    let counter = tokio::spawn(collect_trace_delta_counts(event_rx));
    let trace_sink = std::sync::Arc::new(pl_trace::InMemoryTraceEventSink::new(
        "openai-live-session",
        0,
    ));
    let context = ModelInvocationContext::new(ModelSession::default(), event_tx).with_trace(
        CompletionTraceContext {
            session_id: "openai-live-session".to_string(),
            turn_id: "openai-live-turn".to_string(),
            inference_id: "openai-live-inference".to_string(),
        },
        trace_sink,
    );

    let response = match runtime.complete(openai_request(), context).await {
        Ok(response) => response,
        Err(error) if is_live_auth_or_quota_error(&error.to_string()) => {
            eprintln!("skipping live OpenAI API test: {error}");
            let _ = counter.await;
            return;
        }
        Err(error) => panic!("live OpenAI API request failed: {error}"),
    };
    let counts = counter.await.unwrap();

    let content = response.content.unwrap_or_default();
    assert!(!content.trim().is_empty());
    assert!(
        !content.contains("<final>"),
        "OpenAI Responses native phase output should not require visible tags: {content:?}"
    );
    assert!(response.usage.total_tokens > 0);
    assert!(counts.text > 0, "OpenAI stream should emit text deltas");
}

fn is_live_auth_or_quota_error(error: &str) -> bool {
    error.contains("401")
        || error.contains("403")
        || error.contains("429")
        || error.contains("invalid_api_key")
        || error.contains("insufficient_quota")
}
