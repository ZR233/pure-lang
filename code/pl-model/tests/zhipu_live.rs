use std::collections::HashMap;

use pl_model::{
    CompletionRequest, CompletionResponse, CompletionTimelineContext, ModelProvider, ProviderInfo,
    ReasoningConfig, ReasoningSummary, create_provider,
};
use pl_protocol::{AgentEvent, Message, MessageContent, MessageRole, TimelineDelta};

const ZHIPU_LIVE_ENV_KEY: &str = "API_KEY_ZHIPU";

#[derive(Debug, Default)]
struct TimelineDeltaCounts {
    text: usize,
    thinking: usize,
}

fn user_message(content: &str) -> Message {
    Message {
        role: MessageRole::User,
        content: MessageContent::Text(content.to_string()),
        reasoning_content: None,
        metadata: HashMap::new(),
    }
}

fn live_api_key() -> Option<String> {
    match std::env::var(ZHIPU_LIVE_ENV_KEY) {
        Ok(value) if !value.trim().is_empty() => Some(value),
        _ => {
            eprintln!("{ZHIPU_LIVE_ENV_KEY} is not set; skipping live Zhipu Coding Plan API test");
            None
        }
    }
}

fn zhipu_coding_plan_disabled_request() -> CompletionRequest {
    CompletionRequest {
        model: ProviderInfo::zhipu_coding_plan(None).default_model,
        instructions: Some("请用简短中文回答。".to_string()),
        messages: vec![user_message("请回答：2 + 2 等于几？")],
        tools: Vec::new(),
        tool_choice: "auto".to_string(),
        parallel_tool_calls: false,
        temperature: None,
        max_tokens: Some(128),
        reasoning: Some(ReasoningConfig {
            effort: Some("none".to_string()),
            summary: Some(ReasoningSummary::Disabled),
        }),
        stream: true,
        timeline: None,
    }
}

fn zhipu_coding_plan_thinking_request() -> CompletionRequest {
    CompletionRequest {
        model: ProviderInfo::zhipu_coding_plan(None).default_model,
        instructions: Some("请先思考，最后用一句中文简短作答。".to_string()),
        messages: vec![user_message("比较 9.11 和 9.8 哪个更大？")],
        tools: Vec::new(),
        tool_choice: "auto".to_string(),
        parallel_tool_calls: false,
        temperature: None,
        max_tokens: Some(1024),
        reasoning: Some(ReasoningConfig {
            effort: Some("enabled".to_string()),
            summary: Some(ReasoningSummary::Enabled),
        }),
        stream: true,
        timeline: Some(CompletionTimelineContext {
            session_id: "zhipu-live-session".to_string(),
            turn_id: "zhipu-live-thinking-turn".to_string(),
            inference_id: "zhipu-live-thinking-inference".to_string(),
            starting_sequence: 0,
        }),
    }
}

async fn collect_timeline_delta_counts(
    mut event_rx: tokio::sync::broadcast::Receiver<AgentEvent>,
) -> TimelineDeltaCounts {
    let mut counts = TimelineDeltaCounts::default();
    loop {
        match event_rx.recv().await {
            Ok(AgentEvent::TimelineItemDelta { event }) => match event.delta {
                TimelineDelta::Text { .. } => counts.text += 1,
                TimelineDelta::Thinking { .. } => counts.thinking += 1,
                TimelineDelta::ToolArguments { .. } | TimelineDelta::ToolResult { .. } => {}
            },
            Ok(_) => {}
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                panic!("live Zhipu timeline event receiver lagged by {skipped} events")
            }
        }
    }
    counts
}

async fn run_coding_plan(
    api_key: String,
    request: CompletionRequest,
) -> (CompletionResponse, TimelineDeltaCounts) {
    let mut info = ProviderInfo::zhipu_coding_plan(None);
    info.bearer_token = Some(api_key);
    let provider = create_provider(info).unwrap();
    let (event_tx, event_rx) = tokio::sync::broadcast::channel(4096);
    let counter = tokio::spawn(collect_timeline_delta_counts(event_rx));

    let response = provider.stream_complete(request, event_tx).await.unwrap();
    let counts = counter.await.unwrap();

    (response, counts)
}

#[tokio::test]
async fn zhipu_coding_plan_chat_completion_smoke() {
    let Some(api_key) = live_api_key() else {
        return;
    };

    let (response, _counts) = run_coding_plan(api_key, zhipu_coding_plan_disabled_request()).await;

    assert!(!response.content.unwrap_or_default().trim().is_empty());
}

#[tokio::test]
async fn zhipu_coding_plan_streams_thinking_mode() {
    let Some(api_key) = live_api_key() else {
        return;
    };

    let (response, counts) = run_coding_plan(api_key, zhipu_coding_plan_thinking_request()).await;

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
    assert!(
        counts.thinking > 0,
        "enabled stream should emit thinking deltas"
    );
}
