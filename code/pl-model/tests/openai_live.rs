use std::collections::HashMap;

use pl_model::{
    CompletionRequest, CompletionTraceContext, ModelProvider, ProviderInfo, ReasoningConfig,
    ReasoningSummary, create_provider,
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
        content: MessageContent::Text(content.to_string()),
        reasoning_content: None,
        metadata: HashMap::new(),
    }
}

fn openai_request(model: String) -> CompletionRequest {
    CompletionRequest {
        model,
        instructions: Some(
            "Answer briefly. Put all visible answer text inside <final>...</final>; do not write plain text outside tags."
                .to_string(),
        ),
        messages: vec![user_message("Reply with exactly: <final>ok</final>")],
        tools: Vec::new(),
        tool_choice: "auto".to_string(),
        parallel_tool_calls: false,
        temperature: None,
        max_tokens: Some(128),
        reasoning: Some(ReasoningConfig {
            effort: Some("medium".to_string()),
            summary: Some(ReasoningSummary::Enabled),
        }),
        stream: true,
        trace: Some(CompletionTraceContext {
            session_id: "openai-live-session".to_string(),
            turn_id: "openai-live-turn".to_string(),
            inference_id: "openai-live-inference".to_string(),
            plan_mode: false,
        }),
    }
}

async fn collect_trace_delta_counts(
    mut event_rx: tokio::sync::broadcast::Receiver<AgentEvent>,
) -> TraceDeltaCounts {
    let mut counts = TraceDeltaCounts::default();
    loop {
        match event_rx.recv().await {
            Ok(AgentEvent::TracePartDelta { event }) => match event.delta {
                TraceDelta::Text { .. } => counts.text += 1,
                TraceDelta::Thinking { .. } => counts.thinking += 1,
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
    let mut info = ProviderInfo::openai(base_url);
    info.bearer_token = Some(api_key);
    let model = std::env::var(OPENAI_LIVE_MODEL_ENV_KEY)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| info.default_model.clone());
    let provider = create_provider(info).unwrap();
    let (event_tx, event_rx) = tokio::sync::broadcast::channel(256);
    let counter = tokio::spawn(collect_trace_delta_counts(event_rx));

    let response = match provider
        .stream_complete(openai_request(model), event_tx)
        .await
    {
        Ok(response) => response,
        Err(error) if is_live_auth_or_quota_error(&error.to_string()) => {
            eprintln!("skipping live OpenAI API test: {error}");
            let _ = counter.await;
            return;
        }
        Err(error) => panic!("live OpenAI API request failed: {error}"),
    };
    let counts = counter.await.unwrap();

    assert!(!response.content.unwrap_or_default().trim().is_empty());
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
