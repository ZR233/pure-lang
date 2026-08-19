use std::collections::HashMap;

use pl_model::{
    CompletionRequest, CompletionTraceContext, ModelInvocationContext, ModelRuntime, ModelSession,
    ProviderEndpoint, ReasoningConfig, ReasoningSummary, deepseek_default_model_slugs,
    default_models,
};
use pl_protocol::{Message, MessageContent, MessageRole};
use pl_trace::{AgentEvent, TraceDelta, TraceEvent, TraceEventKind};
use tokio::sync::broadcast::error::RecvError;

const DEEPSEEK_LIVE_ENV_KEY: &str = "API_KEY_DEEPSEEK";

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

fn assistant_message(content: String, reasoning_content: String) -> Message {
    Message {
        role: MessageRole::Assistant,
        content: MessageContent::Text(content),
        reasoning_content: Some(reasoning_content),
        tool_calls: None,
        tool_result: None,
        metadata: HashMap::new(),
    }
}

fn live_api_key() -> Option<String> {
    match std::env::var(DEEPSEEK_LIVE_ENV_KEY) {
        Ok(value) if !value.trim().is_empty() => Some(value),
        _ => {
            eprintln!("{DEEPSEEK_LIVE_ENV_KEY} is not set; skipping live DeepSeek API test");
            None
        }
    }
}

fn deepseek_request(messages: Vec<Message>) -> CompletionRequest {
    CompletionRequest::builder()
        .instructions(
            "请用简短中文回答。所有可见答案必须放在 <final>...</final> 中，不要输出标签之外的普通正文。"
        )
        .messages(messages)
        .max_tokens(2048)
        .reasoning(Some(ReasoningConfig {
            effort: Some("high".to_string()),
            summary: Some(ReasoningSummary::Enabled),
        }))
        .build()
}

struct TurnOutcome {
    content: String,
    reasoning_content: String,
    text_delta_count: usize,
    trace_events: Vec<TraceEvent>,
}

async fn run_turn(api_key: &str, messages: Vec<Message>, turn_id: &str) -> TurnOutcome {
    let mut info = ProviderEndpoint::deepseek(None);
    info.bearer_token = Some(api_key.to_string());
    let model_slug = deepseek_default_model_slugs()[0];
    let model = default_models()
        .into_iter()
        .find(|model| model.slug == model_slug)
        .expect("DeepSeek default model catalog must contain its selected model");
    let runtime = ModelRuntime::new(info, model).unwrap();
    let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(256);

    let event_counter = tokio::spawn(async move {
        let mut text_delta_count = 0;
        loop {
            match event_rx.recv().await {
                Ok(AgentEvent::TracePartDelta { event }) => match event.delta {
                    TraceDelta::Text { .. } => text_delta_count += 1,
                    TraceDelta::Thinking { .. }
                    | TraceDelta::ReasoningContent { .. }
                    | TraceDelta::ToolArguments { .. }
                    | TraceDelta::ToolResult { .. }
                    | TraceDelta::Plan { .. } => {}
                },
                Ok(_) => {}
                Err(RecvError::Closed) => break,
                Err(RecvError::Lagged(_)) => {}
            }
        }
        text_delta_count
    });

    let context = ModelInvocationContext::new(ModelSession::default(), event_tx).with_trace(
        CompletionTraceContext {
            session_id: "live-session".to_string(),
            turn_id: turn_id.to_string(),
            inference_id: format!("{turn_id}-inference"),
            plan_mode: false,
            trace_sequence_base: 0,
        },
    );
    let response = runtime
        .complete(deepseek_request(messages), context.clone())
        .await
        .unwrap();
    let text_delta_count = event_counter.await.unwrap();
    TurnOutcome {
        content: response.content.unwrap_or_default(),
        reasoning_content: response.reasoning_content.unwrap_or_default(),
        text_delta_count,
        trace_events: context.take_trace_events(),
    }
}

/// 提取 trace events 涉及的所有 item_id（去重）。
fn trace_part_ids(events: &[TraceEvent]) -> Vec<String> {
    let mut ids: Vec<String> = events
        .iter()
        .filter_map(|event| match &event.kind {
            TraceEventKind::TracePartStarted { item }
            | TraceEventKind::TracePartCompleted { item }
            | TraceEventKind::TracePartFailed { item, .. } => Some(item.item_id.clone()),
            TraceEventKind::TracePartDelta { event } => Some(event.item_id.clone()),
            _ => None,
        })
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

/// 断言 trace events 的 sequence 严格递增（同 turn 内）。
fn assert_sequences_strictly_increasing(events: &[TraceEvent]) {
    let sequences: Vec<u64> = events.iter().map(|event| event.sequence).collect();
    for window in sequences.windows(2) {
        assert!(
            window[0] < window[1],
            "trace sequence not strictly increasing: {sequences:?}"
        );
    }
}

#[tokio::test]
async fn deepseek_streams_multi_turn_with_thinking_mode() {
    let Some(api_key) = live_api_key() else {
        return;
    };

    let first_messages = vec![user_message(
        "比较 9.11 和 9.8 哪个更大，只给结论和一句理由。",
    )];
    let first = run_turn(&api_key, first_messages.clone(), "turn-1").await;

    assert!(!first.content.trim().is_empty());
    assert!(!first.reasoning_content.trim().is_empty());
    assert!(first.text_delta_count > 0);
    assert!(!first.trace_events.is_empty());
    // turn-1 的所有 item_id 必须以 "turn-1-" 前缀隔离
    let first_ids = trace_part_ids(&first.trace_events);
    assert!(
        first_ids.iter().all(|id| id.starts_with("turn-1-")),
        "turn-1 item ids must be turn-scoped: {first_ids:?}"
    );
    assert_sequences_strictly_increasing(&first.trace_events);

    let second_messages = vec![
        first_messages[0].clone(),
        assistant_message(first.content, first.reasoning_content),
        user_message("继续上一题，把答案改写成不超过 12 个字。"),
    ];
    let second = run_turn(&api_key, second_messages, "turn-2").await;

    assert!(!second.content.trim().is_empty());
    assert!(!second.reasoning_content.trim().is_empty());
    assert!(second.text_delta_count > 0);
    assert!(!second.trace_events.is_empty());
    // turn-2 的所有 item_id 必须以 "turn-2-" 前缀隔离
    let second_ids = trace_part_ids(&second.trace_events);
    assert!(
        second_ids.iter().all(|id| id.starts_with("turn-2-")),
        "turn-2 item ids must be turn-scoped: {second_ids:?}"
    );
    // 跨 turn item_id 绝不交叉（防串台）
    assert!(
        first_ids.iter().all(|id| !second_ids.contains(id)),
        "cross-turn item ids must not overlap: first={first_ids:?} second={second_ids:?}"
    );
    assert_sequences_strictly_increasing(&second.trace_events);
    // 注：pl-model projection sequence 每 turn 从 0 独立分配（不跨 turn 衔接）；
    // 跨 turn 的全局单调由 pl-core store 用 DB envelope.sequence 回填保证（Phase 2）。
    // 此处只验证 pl-model 层 turn_id/item_id 隔离 + 单 turn sequence 单调。
}
