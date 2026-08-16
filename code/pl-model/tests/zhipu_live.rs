use std::collections::HashMap;

use pl_model::{
    CompletionRequest, CompletionResponse, CompletionTraceContext, ModelProvider, ProviderInfo,
    ReasoningConfig, ReasoningSummary, create_provider,
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
    CompletionRequest {
        model: ProviderInfo::zhipu(None).default_model,
        instructions: Some("请用简短中文回答。".to_string()),
        input: vec![user_message("请回答：2 + 2 等于几？").into()],
        tools: Vec::new(),
        tool_choice: "auto".to_string(),
        parallel_tool_calls: false,
        temperature: None,
        max_tokens: Some(128),
        store: None,
        previous_response_id: None,
        prompt_cache_key: None,
        reasoning: Some(ReasoningConfig {
            effort: Some("none".to_string()),
            summary: Some(ReasoningSummary::Disabled),
        }),
        stream: true,
        trace: None,
        transport_session: Default::default(),
    }
}

fn zhipu_thinking_request() -> CompletionRequest {
    CompletionRequest {
        model: ProviderInfo::zhipu(None).default_model,
        instructions: Some(
            "请先思考，最后用一句中文简短作答。所有可见答案必须放在 <final>...</final> 中，不要输出标签之外的普通正文。"
                .to_string(),
        ),
        input: vec![user_message("比较 9.11 和 9.8 哪个更大？").into()],
        tools: Vec::new(),
        tool_choice: "auto".to_string(),
        parallel_tool_calls: false,
        temperature: None,
        max_tokens: Some(1024),
        store: None,
        previous_response_id: None,
        prompt_cache_key: None,
        reasoning: Some(ReasoningConfig {
            effort: Some("enabled".to_string()),
            summary: Some(ReasoningSummary::Enabled),
        }),
        stream: true,
        trace: Some(CompletionTraceContext {
            session_id: "zhipu-live-session".to_string(),
            turn_id: "zhipu-live-thinking-turn".to_string(),
            inference_id: "zhipu-live-thinking-inference".to_string(),
            plan_mode: false,
            trace_sequence_base: 0,
        }),
        transport_session: Default::default(),
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
    let mut info = ProviderInfo::zhipu(None);
    info.bearer_token = Some(api_key);
    let provider = create_provider(info).unwrap();
    let (event_tx, event_rx) = tokio::sync::broadcast::channel(4096);
    let counter = tokio::spawn(collect_trace_delta_counts(event_rx));

    let response = match provider.stream_complete(request, event_tx).await {
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
