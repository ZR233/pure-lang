use std::collections::HashMap;

use pl_model::{
    CompletionRequest, CompletionResponse, CompletionTraceContext, ModelInvocationContext,
    ModelRuntime, ModelSession, ProviderEndpoint, ReasoningConfig, ReasoningSummary,
    default_models, zhipu_default_model_slugs,
};
use pl_protocol::{Message, MessageContent, MessageRole};
use pl_trace::{AgentEvent, TraceDelta};

const ZHIPU_LIVE_ENV_KEY: &str = "API_KEY_ZHIPU";

#[derive(Debug, Default)]
struct TraceDeltaCounts {
    text: usize,
}

fn user_message(content: &str) -> Message {
    Message {
        role: MessageRole::User,
        content: MessageContent::Text(content.to_string()),
        reasoning_content: None,
        tool_calls: None,
        tool_result: None,
        metadata: HashMap::new(),
    }
}

fn live_api_key() -> Option<String> {
    match std::env::var(ZHIPU_LIVE_ENV_KEY) {
        Ok(value) if !value.trim().is_empty() => Some(value),
        _ => {
            eprintln!("{ZHIPU_LIVE_ENV_KEY} is not set; skipping live Zhipu API test");
            None
        }
    }
}

fn zhipu_disabled_request() -> CompletionRequest {
    CompletionRequest::builder()
        .instructions("请用简短中文回答。")
        .messages(vec![user_message("请回答：2 + 2 等于几？")])
        .max_tokens(128)
        .reasoning(Some(ReasoningConfig {
            effort: Some("none".to_string()),
            summary: Some(ReasoningSummary::Disabled),
        }))
        .build()
}

fn zhipu_thinking_request() -> CompletionRequest {
    CompletionRequest::builder()
        .instructions(
            "请先思考，最后用一句中文简短作答。所有可见答案必须放在 <final>...</final> 中，不要输出标签之外的普通正文。"
        )
        .messages(vec![user_message("比较 9.11 和 9.8 哪个更大？")])
        .max_tokens(1024)
        .reasoning(Some(ReasoningConfig {
            effort: Some("enabled".to_string()),
            summary: Some(ReasoningSummary::Enabled),
        }))
        .build()
}

async fn collect_trace_delta_counts(
    mut event_rx: tokio::sync::broadcast::Receiver<AgentEvent>,
) -> TraceDeltaCounts {
    let mut counts = TraceDeltaCounts::default();
    loop {
        match event_rx.recv().await {
            Ok(AgentEvent::TracePartDelta { event }) => match event.delta {
                TraceDelta::Text { .. } => counts.text += 1,
                TraceDelta::Thinking { .. }
                | TraceDelta::ReasoningContent { .. }
                | TraceDelta::ToolArguments { .. }
                | TraceDelta::ToolResult { .. }
                | TraceDelta::Plan { .. } => {}
            },
            Ok(_) => {}
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                panic!("live Zhipu trace event receiver lagged by {skipped} events")
            }
        }
    }
    counts
}

async fn run_zhipu(
    api_key: String,
    request: CompletionRequest,
) -> Option<(CompletionResponse, TraceDeltaCounts)> {
    let mut info = ProviderEndpoint::zhipu(None);
    info.bearer_token = Some(api_key);
    let model_slug = zhipu_default_model_slugs()[0];
    let model = default_models()
        .into_iter()
        .find(|model| model.slug == model_slug)
        .expect("Zhipu default model catalog must contain its selected model");
    let runtime = ModelRuntime::new(info, model).unwrap();
    let (event_tx, event_rx) = tokio::sync::broadcast::channel(4096);
    let counter = tokio::spawn(collect_trace_delta_counts(event_rx));
    let trace_sink = std::sync::Arc::new(pl_trace::InMemoryTraceEventSink::new(
        "zhipu-live-session",
        0,
    ));
    let context = ModelInvocationContext::new(ModelSession::default(), event_tx).with_trace(
        CompletionTraceContext {
            session_id: "zhipu-live-session".to_string(),
            turn_id: "zhipu-live-turn".to_string(),
            inference_id: "zhipu-live-inference".to_string(),
        },
        trace_sink,
    );

    let response = match runtime.complete(request, context).await {
        Ok(response) => response,
        Err(error) if is_live_quota_error(&error.to_string()) => {
            eprintln!("skipping live Zhipu API test: {error}");
            let _ = counter.await;
            return None;
        }
        Err(error) => panic!("live Zhipu API request failed: {error}"),
    };
    let counts = counter.await.unwrap();

    Some((response, counts))
}

fn is_live_quota_error(error: &str) -> bool {
    error.contains("429") || error.contains("余额不足") || error.contains("无可用资源包")
}

#[tokio::test]
async fn zhipu_chat_completion_smoke() {
    let Some(api_key) = live_api_key() else {
        return;
    };

    let Some((response, _counts)) = run_zhipu(api_key, zhipu_disabled_request()).await else {
        return;
    };

    assert!(!response.content.unwrap_or_default().trim().is_empty());
}

#[tokio::test]
async fn zhipu_streams_thinking_mode() {
    let Some(api_key) = live_api_key() else {
        return;
    };

    let Some((response, counts)) = run_zhipu(api_key, zhipu_thinking_request()).await else {
        return;
    };

    assert!(!response.content.unwrap_or_default().trim().is_empty());
    assert!(
        !response
            .reasoning_content
            .unwrap_or_default()
            .trim()
            .is_empty(),
        "enabled thinking should return reasoning_content"
    );
    assert!(counts.text > 0, "enabled stream should emit text deltas");
}
