use serde::Deserialize;

use crate::completion::stream::event::{ModelStreamEvent, ToolInputDeltaPayload};
use crate::runtime::openai::identity::responses_tool_identity;
use crate::runtime::openai::usage::ProviderTokenUsage;
use pl_trace::TraceTextChannel;

mod decoder;
mod item;

pub(crate) use decoder::OpenAiStreamDecoder;
use item::{
    output_item_native_context, output_item_tool_completed, output_item_tool_started,
    web_search_lifecycle_event,
};

/// SSE 流事件原始结构（从 JSON 解析）
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct SseStreamEvent {
    pub id: Option<String>,
    pub model: Option<String>,
    #[serde(rename = "type")]
    #[serde(default)]
    pub kind: String,
    pub delta: Option<String>,
    pub item: Option<serde_json::Value>,
    pub item_id: Option<String>,
    pub call_id: Option<String>,
    pub response: Option<serde_json::Value>,
    pub error: Option<serde_json::Value>,
    pub code: Option<String>,
    pub message: Option<String>,
    pub retry_after_ms: Option<u64>,
    pub status: Option<serde_json::Value>,
    pub status_code: Option<serde_json::Value>,
    pub summary_index: Option<i64>,
    pub content_index: Option<i64>,
    pub choices: Option<Vec<ChatStreamChoice>>,
    pub usage: Option<ProviderTokenUsage>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct ChatStreamChoice {
    pub delta: ChatStreamDelta,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct ChatStreamDelta {
    pub content: Option<String>,
    pub reasoning_content: Option<String>,
    pub tool_calls: Option<Vec<ChatStreamToolCallDelta>>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct ChatStreamToolCallDelta {
    pub index: Option<usize>,
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub function: Option<ChatStreamFunctionDelta>,
    pub custom: Option<ChatStreamCustomDelta>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct ChatStreamFunctionDelta {
    pub name: Option<String>,
    pub arguments: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct ChatStreamCustomDelta {
    pub name: Option<String>,
    pub input: Option<String>,
}

const DEFAULT_TEXT_ID: &str = "final";
const DEFAULT_REASONING_ID: &str = "thinking";
/// Chat Completions chunk 的 delta 解析；非 chat 事件（无 choices）返回 `None`。
fn chat_choice_events(event: &SseStreamEvent) -> Option<Vec<ModelStreamEvent>> {
    let choices = event.choices.as_ref()?;
    let mut events = Vec::new();
    if let Some(usage) = event
        .usage
        .as_ref()
        .and_then(ProviderTokenUsage::to_chat_usage)
    {
        events.push(ModelStreamEvent::Usage(usage));
    }
    let Some(choice) = choices.first() else {
        return Some(events);
    };
    if let Some(delta) = &choice.delta.reasoning_content
        && !delta.is_empty()
    {
        events.push(ModelStreamEvent::ReasoningRawDelta {
            id: DEFAULT_REASONING_ID.to_string(),
            content_index: 0,
            delta: delta.clone(),
        });
    }

    if let Some(content) = &choice.delta.content
        && !content.is_empty()
    {
        events.push(ModelStreamEvent::text_delta(
            DEFAULT_TEXT_ID.to_string(),
            TraceTextChannel::Final,
            content.clone(),
        ));
    }

    if let Some(tool_calls) = &choice.delta.tool_calls {
        for tool_call in tool_calls {
            let index = tool_call.index.unwrap_or_default();
            let stream_id = Some(format!("chat_tool_call:{index}"));
            let item_id = tool_call.id.clone().unwrap_or_default();
            // Chat Completions 只暴露 item id；确定性赋 call_id = item_id。
            let call_id = (!item_id.is_empty()).then(|| item_id.clone());
            if let Some(custom) = &tool_call.custom {
                events.push(ModelStreamEvent::ToolInputDelta {
                    stream_id,
                    item_id,
                    call_id,
                    name: custom.name.clone(),
                    payload_delta: ToolInputDeltaPayload::CustomInput(
                        custom.input.clone().unwrap_or_default(),
                    ),
                });
                continue;
            }
            if let Some(function) = &tool_call.function {
                events.push(ModelStreamEvent::ToolInputDelta {
                    stream_id,
                    item_id,
                    call_id,
                    name: function.name.clone(),
                    payload_delta: ToolInputDeltaPayload::FunctionArguments(
                        function.arguments.clone().unwrap_or_default(),
                    ),
                });
            }
        }
    }

    (!events.is_empty()).then_some(events)
}

/// 将原始 SSE 事件解析为 canonical stream event。
pub fn process_sse_events(event: &SseStreamEvent) -> Vec<ModelStreamEvent> {
    match process_sse_event(event) {
        Some(StreamEventBatch::Single(event)) => vec![event],
        Some(StreamEventBatch::Many(events)) => events,
        None => Vec::new(),
    }
}

enum StreamEventBatch {
    Single(ModelStreamEvent),
    Many(Vec<ModelStreamEvent>),
}

fn process_sse_event(event: &SseStreamEvent) -> Option<StreamEventBatch> {
    if let Some(event) = web_search_lifecycle_event(event) {
        return Some(StreamEventBatch::Single(event));
    }
    if let Some(events) = chat_choice_events(event) {
        return Some(StreamEventBatch::Many(events));
    }

    match event.kind.as_str() {
        "response.created" => Some(StreamEventBatch::Single(
            ModelStreamEvent::ResponseStarted {
                response_id: event
                    .response
                    .as_ref()
                    .and_then(|r| r.get("id")?.as_str().map(String::from)),
            },
        )),

        "response.output_text.delta" => event.delta.as_ref().map(|d| {
            StreamEventBatch::Single(ModelStreamEvent::text_delta(
                event
                    .item_id
                    .clone()
                    .unwrap_or_else(|| DEFAULT_TEXT_ID.to_string()),
                TraceTextChannel::Final,
                d.clone(),
            ))
        }),

        "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
            event.delta.as_ref().map(|d| {
                if event.kind == "response.reasoning_summary_text.delta" {
                    StreamEventBatch::Single(ModelStreamEvent::reasoning_summary_delta(
                        event
                            .item_id
                            .clone()
                            .unwrap_or_else(|| DEFAULT_REASONING_ID.to_string()),
                        event.summary_index.unwrap_or(0).max(0) as u32,
                        d.clone(),
                    ))
                } else {
                    StreamEventBatch::Single(ModelStreamEvent::ReasoningRawDelta {
                        id: event
                            .item_id
                            .clone()
                            .unwrap_or_else(|| DEFAULT_REASONING_ID.to_string()),
                        content_index: event.content_index.unwrap_or(0).max(0) as u32,
                        delta: d.clone(),
                    })
                }
            })
        }

        "response.function_call_arguments.delta" => {
            let (item_id, call_id) = responses_tool_identity(
                event.item_id.as_deref(),
                event.call_id.as_deref(),
                &event.kind,
            );
            Some(StreamEventBatch::Single(ModelStreamEvent::ToolInputDelta {
                stream_id: None,
                item_id,
                call_id: Some(call_id),
                name: None,
                payload_delta: ToolInputDeltaPayload::FunctionArguments(
                    event.delta.clone().unwrap_or_default(),
                ),
            }))
        }

        "response.custom_tool_call_input.delta" => {
            let (item_id, call_id) = responses_tool_identity(
                event.item_id.as_deref(),
                event.call_id.as_deref(),
                &event.kind,
            );
            Some(StreamEventBatch::Single(ModelStreamEvent::ToolInputDelta {
                stream_id: None,
                item_id,
                call_id: Some(call_id),
                name: None,
                payload_delta: ToolInputDeltaPayload::CustomInput(
                    event.delta.clone().unwrap_or_default(),
                ),
            }))
        }

        "response.output_item.added" => event
            .item
            .as_ref()
            .and_then(output_item_tool_started)
            .map(StreamEventBatch::Single),

        "response.output_item.done" => event.item.as_ref().and_then(|item| {
            let mut events = output_item_tool_completed(item).unwrap_or_default();
            if let Some(native) = output_item_native_context(item) {
                events.push(native);
            }
            (!events.is_empty()).then_some(StreamEventBatch::Many(events))
        }),

        "response.completed" => {
            let usage = event
                .response
                .as_ref()
                .and_then(|response| response.get("usage"))
                .and_then(ProviderTokenUsage::from_value)
                .and_then(|usage| usage.to_responses_usage());
            let mut events = Vec::new();
            if let Some(usage) = usage {
                events.push(ModelStreamEvent::Usage(usage));
            }
            events.push(ModelStreamEvent::Completed {
                response_id: event
                    .response
                    .as_ref()
                    .and_then(|r| r.get("id")?.as_str().map(String::from)),
            });
            Some(StreamEventBatch::Many(events))
        }

        "response.failed" => {
            let response = event.response.as_ref();
            let failure = provider_failure_event(
                response.and_then(|response| response.get("error")),
                response.and_then(|response| response.get("code").and_then(|value| value.as_str())),
                response
                    .and_then(|response| response.get("message").and_then(|value| value.as_str())),
                response.and_then(provider_status),
                response.and_then(|response| {
                    response
                        .get("retry_after_ms")
                        .and_then(|value| value.as_u64())
                }),
                "response failed",
            );
            let mut events = Vec::new();
            if let Some(usage) = response
                .and_then(|r| r.get("usage"))
                .and_then(ProviderTokenUsage::from_value)
                .and_then(|usage| usage.to_responses_usage())
            {
                events.push(ModelStreamEvent::Usage(usage));
            }
            events.push(failure);
            Some(StreamEventBatch::Many(events))
        }

        "response.incomplete" => {
            let response = event.response.as_ref();
            let reason = response
                .and_then(|response| response.get("incomplete_details")?.get("reason")?.as_str());
            let failure = provider_failure_event(
                response.and_then(|response| response.get("error")),
                reason,
                None,
                response.and_then(provider_status),
                None,
                "response incomplete",
            );
            let mut events = Vec::new();
            if let Some(usage) = response
                .and_then(|r| r.get("usage"))
                .and_then(ProviderTokenUsage::from_value)
                .and_then(|usage| usage.to_responses_usage())
            {
                events.push(ModelStreamEvent::Usage(usage));
            }
            events.push(failure);
            Some(StreamEventBatch::Many(events))
        }

        "error" => Some(StreamEventBatch::Single(provider_failure_event(
            event.error.as_ref(),
            event.code.as_deref(),
            event.message.as_deref(),
            event
                .status
                .as_ref()
                .and_then(provider_status_value)
                .or_else(|| event.status_code.as_ref().and_then(provider_status_value)),
            event.retry_after_ms,
            "provider stream failed",
        ))),

        _ => None,
    }
}

fn provider_failure_event(
    error: Option<&serde_json::Value>,
    fallback_code: Option<&str>,
    fallback_message: Option<&str>,
    fallback_status: Option<u16>,
    fallback_retry_after_ms: Option<u64>,
    default_message: &str,
) -> ModelStreamEvent {
    ModelStreamEvent::Failed {
        code: error
            .and_then(|error| error.get("code"))
            .and_then(serde_json::Value::as_str)
            .or(fallback_code)
            .map(ToString::to_string),
        http_status: error.and_then(provider_status).or(fallback_status),
        retry_after_ms: error
            .and_then(|error| error.get("retry_after_ms"))
            .and_then(serde_json::Value::as_u64)
            .or(fallback_retry_after_ms),
        message: error
            .and_then(|error| error.get("message"))
            .and_then(serde_json::Value::as_str)
            .or(fallback_message)
            .unwrap_or(default_message)
            .to_string(),
    }
}

fn provider_status(value: &serde_json::Value) -> Option<u16> {
    value
        .get("status")
        .or_else(|| value.get("status_code"))
        .and_then(provider_status_value)
}

fn provider_status_value(value: &serde_json::Value) -> Option<u16> {
    value
        .as_u64()
        .and_then(|status| u16::try_from(status).ok())
        .or_else(|| value.as_str()?.parse().ok())
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::completion::stream::event::{ModelBlockField, ModelBlockKind, ToolInputPayloadKind};

    fn single_event(event: &SseStreamEvent) -> Option<ModelStreamEvent> {
        let events = process_sse_events(event);
        assert!(events.len() <= 1, "expected at most one event: {events:?}");
        events.into_iter().next()
    }

    fn chat_event(delta: serde_json::Value) -> SseStreamEvent {
        serde_json::from_value(serde_json::json!({
            "choices": [{
                "delta": delta,
                "finish_reason": null
            }]
        }))
        .unwrap()
    }

    #[test]
    fn process_chat_reasoning_content_as_thinking_delta() {
        let event = chat_event(serde_json::json!({
            "reasoning_content": "先比较整数位。"
        }));

        match single_event(&event) {
            Some(ModelStreamEvent::ReasoningRawDelta { delta, .. }) => {
                assert_eq!(delta, "先比较整数位。");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn process_chat_content_as_output_text_delta() {
        let event = chat_event(serde_json::json!({
            "content": "9.11 更大。"
        }));

        match single_event(&event) {
            Some(ModelStreamEvent::BlockDelta {
                field: ModelBlockField::Text,
                delta,
                ..
            }) => {
                assert_eq!(delta, "9.11 更大。");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn deepseek_web_search_sse_preserves_lifecycle_actions_and_native_context() {
        let events = [
            serde_json::json!({
                "type": "response.web_search_call.searching",
                "item_id": "search_1"
            }),
            serde_json::json!({
                "type": "response.output_item.added",
                "item": {
                    "type": "web_search_call",
                    "id": "search_1",
                    "action": {"type": "search", "query": "DeepSeek Responses API"}
                }
            }),
            serde_json::json!({
                "type": "response.output_item.done",
                "item": {
                    "type": "web_search_call",
                    "id": "search_1",
                    "action": {"type": "open_page", "url": "https://api-docs.deepseek.com"},
                    "results": [{"url": "https://api-docs.deepseek.com", "opaque": {"rank": 1}}]
                }
            }),
            serde_json::json!({
                "type": "response.output_item.done",
                "item": {
                    "type": "web_search_call",
                    "id": "find_1",
                    "action": {
                        "type": "find_in_page",
                        "url": "https://api-docs.deepseek.com",
                        "pattern": "web_search"
                    }
                }
            }),
            serde_json::json!({
                "type": "response.web_search_call.completed",
                "item_id": "search_1"
            }),
        ];
        let decoded = events
            .into_iter()
            .flat_map(|event| {
                let event: SseStreamEvent = serde_json::from_value(event).unwrap();
                process_sse_events(&event)
            })
            .collect::<Vec<_>>();

        assert!(matches!(
            &decoded[0],
            ModelStreamEvent::WebSearchStarted { item_id, .. } if item_id == "search_1"
        ));
        assert!(decoded.iter().any(|event| matches!(
            event,
            ModelStreamEvent::WebSearchStarted {
                action: crate::completion::WebSearchAction::Search { query: Some(query), .. },
                ..
            } if query == "DeepSeek Responses API"
        )));
        assert!(decoded.iter().any(|event| matches!(
            event,
            ModelStreamEvent::WebSearchCompleted {
                action: crate::completion::WebSearchAction::OpenPage { url: Some(url) },
                results: Some(results),
                ..
            } if url == "https://api-docs.deepseek.com" && results[0]["opaque"]["rank"] == 1
        )));
        assert!(decoded.iter().any(|event| matches!(
        event,
        ModelStreamEvent::WebSearchCompleted {
            action: crate::completion::WebSearchAction::FindInPage { pattern: Some(pattern), .. },
            ..
        } if pattern == "web_search"
    )));
        assert!(decoded.iter().any(|event| matches!(
            event,
            ModelStreamEvent::ResponsesContextItem { item }
                if item.value["results"][0]["opaque"]["rank"] == 1
        )));
    }

    #[test]
    fn process_chat_reasoning_and_content_from_same_chunk() {
        let event = chat_event(serde_json::json!({
            "reasoning_content": "先比较整数位。",
            "content": "<final>9.11 更大。</final>"
        }));

        match process_sse_events(&event).as_slice() {
            [
                ModelStreamEvent::ReasoningRawDelta {
                    delta: reasoning, ..
                },
                ModelStreamEvent::BlockDelta {
                    field: ModelBlockField::Text,
                    delta: content,
                    ..
                },
            ] => {
                assert_eq!(reasoning, "先比较整数位。");
                assert_eq!(content, "<final>9.11 更大。</final>");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn process_responses_marks_summary_and_raw_reasoning() {
        let summary: SseStreamEvent = serde_json::from_value(serde_json::json!({
            "type": "response.reasoning_summary_text.delta",
            "item_id": "rs_1",
            "summary_index": 1,
            "delta": "摘要"
        }))
        .unwrap();
        let raw: SseStreamEvent = serde_json::from_value(serde_json::json!({
            "type": "response.reasoning_text.delta",
            "item_id": "rt_1",
            "content_index": 2,
            "delta": "内部推理"
        }))
        .unwrap();

        match single_event(&summary) {
            Some(ModelStreamEvent::BlockDelta {
                id,
                kind: ModelBlockKind::ReasoningSummary,
                field: ModelBlockField::ReasoningSummary,
                section_index,
                delta,
            }) => {
                assert_eq!(id, "rs_1");
                assert_eq!(section_index, Some(1));
                assert_eq!(delta, "摘要");
            }
            other => panic!("unexpected event: {other:?}"),
        }
        match single_event(&raw) {
            Some(ModelStreamEvent::ReasoningRawDelta {
                id,
                content_index,
                delta,
            }) => {
                assert_eq!(id, "rt_1");
                assert_eq!(content_index, 2);
                assert_eq!(delta, "内部推理");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn process_responses_custom_tool_delta() {
        let event: SseStreamEvent = serde_json::from_value(serde_json::json!({
            "type": "response.custom_tool_call_input.delta",
            "item_id": "ctc_1",
            "call_id": "call_1",
            "delta": "*** Begin Patch\n"
        }))
        .unwrap();

        match single_event(&event) {
            Some(ModelStreamEvent::ToolInputDelta {
                item_id,
                call_id,
                payload_delta: ToolInputDeltaPayload::CustomInput(delta),
                ..
            }) => {
                assert_eq!(item_id, "ctc_1");
                assert_eq!(call_id.as_deref(), Some("call_1"));
                assert_eq!(delta, "*** Begin Patch\n");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn process_chat_custom_tool_delta() {
        let event = chat_event(serde_json::json!({
            "tool_calls": [{
                "index": 0,
                "id": "call_1",
                "type": "custom",
                "custom": {
                    "name": "apply_patch",
                    "input": "*** Begin Patch\n"
                }
            }]
        }));

        match single_event(&event) {
            Some(ModelStreamEvent::ToolInputDelta {
                stream_id,
                item_id,
                name,
                payload_delta: ToolInputDeltaPayload::CustomInput(delta),
                ..
            }) => {
                assert_eq!(stream_id.as_deref(), Some("chat_tool_call:0"));
                assert_eq!(item_id, "call_1");
                assert_eq!(name.as_deref(), Some("apply_patch"));
                assert_eq!(delta, "*** Begin Patch\n");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn process_chat_followup_tool_delta_keeps_stream_id_without_item_id() {
        let event = chat_event(serde_json::json!({
            "tool_calls": [{
                "index": 0,
                "function": {
                    "arguments": "{\"path\":\"Cargo.toml\"}"
                }
            }]
        }));

        match single_event(&event) {
            Some(ModelStreamEvent::ToolInputDelta {
                stream_id,
                item_id,
                name,
                payload_delta: ToolInputDeltaPayload::FunctionArguments(delta),
                ..
            }) => {
                assert_eq!(stream_id.as_deref(), Some("chat_tool_call:0"));
                assert_eq!(item_id, "");
                assert_eq!(name, None);
                assert_eq!(delta, "{\"path\":\"Cargo.toml\"}");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn process_responses_output_item_added_captures_tool_name() {
        let event: SseStreamEvent = serde_json::from_value(serde_json::json!({
            "type": "response.output_item.added",
            "item": {
                "type": "custom_tool_call",
                "id": "ctc_1",
                "call_id": "call_1",
                "name": "apply_patch"
            }
        }))
        .unwrap();

        match single_event(&event) {
            Some(ModelStreamEvent::ToolInputStarted {
                item_id,
                call_id,
                name,
                payload_kind,
                ..
            }) => {
                assert_eq!(item_id, "ctc_1");
                assert_eq!(call_id.as_deref(), Some("call_1"));
                assert_eq!(name.as_deref(), Some("apply_patch"));
                assert_eq!(payload_kind, ToolInputPayloadKind::CustomInput);
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn responses_id_only_delta_populates_call_id() {
        let event: SseStreamEvent = serde_json::from_value(serde_json::json!({
            "type": "response.function_call_arguments.delta",
            "item_id": "fc_1",
            "delta": "{}"
        }))
        .unwrap();

        assert!(matches!(
            single_event(&event),
            Some(ModelStreamEvent::ToolInputDelta { item_id, call_id: Some(call_id), .. })
                if item_id == "fc_1" && call_id == "fc_1"
        ));
    }
}
