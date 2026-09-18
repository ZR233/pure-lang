use std::collections::HashMap;

use pl_model::completion::{
    CompletionRequest, CompletionResponse, CompletionTraceContext, ReasoningConfig,
    ReasoningSummary,
};
use pl_model::config::{ModelCatalogId, builtin_model_catalog};
use pl_model::model::{default_models, zhipu_default_model_slugs};
use pl_model::provider::ProviderEndpoint;
use pl_model::runtime::{ModelInvocationContext, ModelRuntime, ModelSession};

use pl_protocol::trace::{AgentEvent, TraceDelta};
use pl_protocol::{Message, MessageContent, MessageRole};

const ZHIPU_LIVE_ENV_KEY: &str = "ZAI_API_KEY";

#[derive(Debug, Default)]
struct TraceDeltaCounts {
    text: usize,
}

fn user_message(content: &str) -> Message {
    Message {
        presentation: Default::default(),
        role: MessageRole::User,
        content: MessageContent::text(content.to_string()),
        reasoning_content: None,
        tool_calls: None,
        tool_result: None,
        metadata: HashMap::new(),
    }
}

fn live_api_key() -> String {
    std::env::var(ZHIPU_LIVE_ENV_KEY)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .expect("explicit live acceptance requires ZAI_API_KEY")
}

fn zhipu_disabled_request() -> CompletionRequest {
    CompletionRequest::builder()
        .instructions("请用简短中文回答。")
        .messages(vec![user_message("请回答：2 + 2 等于几？")])
        .max_tokens(128)
        .reasoning(Some(ReasoningConfig {
            effort: Some("low".to_string()),
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
            effort: Some("high".to_string()),
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
                | TraceDelta::ToolResult { .. } => {}
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
    let trace_sink = std::sync::Arc::new(pl_protocol::trace::InMemoryTraceEventSink::new(
        "zhipu-live-session",
        0,
    ));
    let context = ModelInvocationContext::new(ModelSession::default())
        .with_events(event_tx)
        .with_trace(
            CompletionTraceContext {
                session_id: "zhipu-live-session".to_string(),
                turn_id: "zhipu-live-turn".to_string(),
                inference_id: "zhipu-live-inference".to_string(),
            },
            trace_sink,
        );

    let response = match runtime.complete(request, context).await {
        Ok(response) => response,
        Err(error) => panic!("live Zhipu API request failed: {error}"),
    };
    let counts = counter.await.unwrap();

    Some((response, counts))
}

#[tokio::test]
async fn zhipu_chat_completion_smoke() {
    let api_key = live_api_key();

    let Some((response, _counts)) = run_zhipu(api_key, zhipu_disabled_request()).await else {
        return;
    };

    assert!(!response.content.unwrap_or_default().trim().is_empty());
}

#[tokio::test]
async fn zhipu_streams_thinking_mode() {
    let api_key = live_api_key();

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

/// Coding Plan 的 OpenAI Response 协议端点冒烟：真实注册目录 + `reasoning.effort` wire。
#[tokio::test]
async fn zhipu_coding_plan_responses_smoke() {
    let api_key = live_api_key();

    let mut endpoint = ProviderEndpoint::zhipu_coding_plan(None);
    endpoint.bearer_token = Some(api_key);
    let model = builtin_model_catalog(&ModelCatalogId::new("zhipu-responses").unwrap())
        .expect("registered coding plan catalog")
        .models
        .into_iter()
        .find(|model| model.slug == "glm-5.3")
        .expect("coding plan catalog contains glm-5.3");
    assert_eq!(
        model.binding.transport.protocol,
        pl_model::provider::ProviderWireProtocol::Responses
    );
    let runtime = ModelRuntime::new(endpoint, model).unwrap();
    let request = CompletionRequest::builder()
        .instructions("请用简短中文回答。")
        .messages(vec![user_message("请回答：3 + 4 等于几？")])
        .tools(vec![pl_protocol::ToolSpec::function(
            "add",
            "计算两个整数的和",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "left": {"type": "integer"},
                    "right": {"type": "integer"}
                },
                "required": ["left", "right"]
            }),
        )])
        .max_tokens(256)
        .reasoning(Some(ReasoningConfig {
            effort: Some("low".to_string()),
            summary: Some(ReasoningSummary::Disabled),
        }))
        .build();
    let (event_tx, event_rx) = tokio::sync::broadcast::channel(4096);
    let counter = tokio::spawn(collect_trace_delta_counts(event_rx));
    let trace_sink = std::sync::Arc::new(pl_protocol::trace::InMemoryTraceEventSink::new(
        "zhipu-live-session",
        0,
    ));
    let response = runtime
        .complete(
            request,
            ModelInvocationContext::new(ModelSession::default())
                .with_events(event_tx)
                .with_trace(
                    CompletionTraceContext {
                        session_id: "zhipu-live-session".to_string(),
                        turn_id: "zhipu-live-turn".to_string(),
                        inference_id: "zhipu-live-inference".to_string(),
                    },
                    trace_sink,
                ),
        )
        .await
        .unwrap_or_else(|error| panic!("live coding plan Responses request failed: {error}"));
    let counts = counter.await.unwrap();

    let content = response.content.unwrap_or_default();
    let called_add = response.tool_calls.iter().any(|call| call.name == "add");
    assert!(
        content.contains('7') || called_add,
        "the answer must solve the prompt directly or through the add tool: {content:?} calls={}",
        response.tool_calls.len()
    );
    if !called_add {
        assert!(counts.text > 0, "responses stream should emit text deltas");
    }
}
