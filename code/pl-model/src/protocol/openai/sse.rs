use std::collections::HashMap;

use serde::Deserialize;
use serde_json::Value;

use crate::request::TokenUsage;
use crate::stream::event::{ModelStreamEvent, ToolInputDeltaPayload, ToolInputPayloadKind};
use pl_trace::TraceTextChannel;

/// SSE 流事件原始结构（从 JSON 解析）
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct SseStreamEvent {
    #[serde(rename = "type")]
    #[serde(default)]
    pub kind: String,
    pub delta: Option<String>,
    pub item: Option<serde_json::Value>,
    pub item_id: Option<String>,
    pub call_id: Option<String>,
    pub response: Option<serde_json::Value>,
    pub summary_index: Option<i64>,
    pub content_index: Option<i64>,
    pub choices: Option<Vec<ChatStreamChoice>>,
    pub usage: Option<ChatTokenUsage>,
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

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct ChatTokenUsage {
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub prompt_tokens_details: Option<serde_json::Value>,
    pub input_tokens_details: Option<serde_json::Value>,
    pub prompt_cache_hit_tokens: Option<u64>,
}

pub(crate) type StreamEvent = ModelStreamEvent;
pub(crate) type ToolCallDeltaPayload = ToolInputDeltaPayload;

const DEFAULT_TEXT_ID: &str = "final";
const DEFAULT_REASONING_ID: &str = "thinking";

/// Stateful OpenAI stream decoder.
///
/// Responses output text deltas do not carry the assistant message phase, so
/// the decoder remembers `response.output_item.added` metadata for the stream.
pub(crate) struct OpenAiStreamDecoder {
    use_native_phases: bool,
    text_channels: HashMap<String, TraceTextChannel>,
}

impl OpenAiStreamDecoder {
    pub(crate) fn new(use_native_phases: bool) -> Self {
        Self {
            use_native_phases,
            text_channels: HashMap::new(),
        }
    }

    pub(crate) fn decode(&mut self, event: &SseStreamEvent) -> Vec<StreamEvent> {
        if !self.use_native_phases {
            return process_sse_events(event);
        }

        match event.kind.as_str() {
            "response.output_item.added" => {
                if let Some((item_id, channel)) = assistant_message_identity(event.item.as_ref()) {
                    self.text_channels.insert(item_id.clone(), channel);
                    return vec![StreamEvent::TextStarted {
                        id: item_id,
                        channel,
                    }];
                }
            }
            "response.output_text.delta" => {
                let item_id = event
                    .item_id
                    .clone()
                    .unwrap_or_else(|| DEFAULT_TEXT_ID.to_string());
                let channel = self
                    .text_channels
                    .get(&item_id)
                    .copied()
                    .unwrap_or(TraceTextChannel::Final);
                if let Some(delta) = event.delta.clone() {
                    return vec![StreamEvent::TextDelta {
                        id: item_id,
                        channel,
                        delta,
                    }];
                }
                return Vec::new();
            }
            "response.output_item.done" => {
                if let Some(item) = event.item.as_ref()
                    && let Some((item_id, item_channel)) = assistant_message_identity(Some(item))
                {
                    let channel = self.text_channels.remove(&item_id).unwrap_or(item_channel);
                    let authoritative_text = assistant_message_text(item);
                    return vec![StreamEvent::TextCompleted {
                        id: item_id,
                        channel,
                        authoritative_text,
                    }];
                }
            }
            _ => {}
        }

        process_sse_events(event)
    }
}

/// 将原始 SSE 事件解析为 canonical stream event。
pub fn process_sse_events(event: &SseStreamEvent) -> Vec<StreamEvent> {
    match process_sse_event(event) {
        Some(StreamEventBatch::Single(event)) => vec![event],
        Some(StreamEventBatch::Many(events)) => events,
        None => Vec::new(),
    }
}

enum StreamEventBatch {
    Single(StreamEvent),
    Many(Vec<StreamEvent>),
}

fn process_sse_event(event: &SseStreamEvent) -> Option<StreamEventBatch> {
    if let Some(choice) = event.choices.as_ref().and_then(|choices| choices.first()) {
        let mut events = Vec::new();
        if let Some(delta) = &choice.delta.reasoning_content
            && !delta.is_empty()
        {
            events.push(StreamEvent::ReasoningDelta {
                id: DEFAULT_REASONING_ID.to_string(),
                chunk_index: 0,
                delta: delta.clone(),
            });
        }

        if let Some(content) = &choice.delta.content
            && !content.is_empty()
        {
            events.push(StreamEvent::TextDelta {
                id: DEFAULT_TEXT_ID.to_string(),
                channel: TraceTextChannel::Final,
                delta: content.clone(),
            });
        }

        if let Some(tool_calls) = &choice.delta.tool_calls {
            for tool_call in tool_calls {
                let index = tool_call.index.unwrap_or_default();
                let stream_id = Some(format!("chat_tool_call:{index}"));
                let item_id = tool_call.id.clone().unwrap_or_default();
                if let Some(custom) = &tool_call.custom {
                    events.push(StreamEvent::ToolInputDelta {
                        stream_id,
                        item_id,
                        call_id: None,
                        name: custom.name.clone(),
                        payload_delta: ToolCallDeltaPayload::CustomInput(
                            custom.input.clone().unwrap_or_default(),
                        ),
                    });
                    continue;
                }
                if let Some(function) = &tool_call.function {
                    events.push(StreamEvent::ToolInputDelta {
                        stream_id,
                        item_id,
                        call_id: None,
                        name: function.name.clone(),
                        payload_delta: ToolCallDeltaPayload::FunctionArguments(
                            function.arguments.clone().unwrap_or_default(),
                        ),
                    });
                }
            }
        }

        if choice.finish_reason.is_some() {
            let usage = event.usage.as_ref().map(|u| TokenUsage {
                prompt_tokens: u.prompt_tokens.unwrap_or(0),
                completion_tokens: u.completion_tokens.unwrap_or(0),
                total_tokens: u.total_tokens.unwrap_or(0),
                cached_prompt_tokens: u
                    .prompt_cache_hit_tokens
                    .or_else(|| {
                        u.prompt_tokens_details
                            .as_ref()
                            .and_then(cached_tokens_from_details)
                    })
                    .or_else(|| {
                        u.input_tokens_details
                            .as_ref()
                            .and_then(cached_tokens_from_details)
                    })
                    .unwrap_or(0),
            });
            if let Some(usage) = usage {
                events.push(StreamEvent::Usage(usage));
            }
            events.push(StreamEvent::Completed { response_id: None });
        }

        if !events.is_empty() {
            return Some(StreamEventBatch::Many(events));
        }
    }

    match event.kind.as_str() {
        "response.created" => Some(StreamEventBatch::Single(StreamEvent::StepStarted {
            response_id: event
                .response
                .as_ref()
                .and_then(|r| r.get("id")?.as_str().map(String::from)),
        })),

        "response.output_text.delta" => event.delta.as_ref().map(|d| {
            StreamEventBatch::Single(StreamEvent::TextDelta {
                id: event
                    .item_id
                    .clone()
                    .unwrap_or_else(|| DEFAULT_TEXT_ID.to_string()),
                channel: TraceTextChannel::Final,
                delta: d.clone(),
            })
        }),

        "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
            event.delta.as_ref().map(|d| {
                StreamEventBatch::Single(StreamEvent::ReasoningDelta {
                    id: event
                        .item_id
                        .clone()
                        .unwrap_or_else(|| DEFAULT_REASONING_ID.to_string()),
                    chunk_index: event
                        .summary_index
                        .or(event.content_index)
                        .unwrap_or(0)
                        .max(0) as u32,
                    delta: d.clone(),
                })
            })
        }

        "response.function_call_arguments.delta" => {
            Some(StreamEventBatch::Single(StreamEvent::ToolInputDelta {
                stream_id: None,
                item_id: event.item_id.clone().unwrap_or_default(),
                call_id: event.call_id.clone(),
                name: None,
                payload_delta: ToolCallDeltaPayload::FunctionArguments(
                    event.delta.clone().unwrap_or_default(),
                ),
            }))
        }

        "response.custom_tool_call_input.delta" => {
            Some(StreamEventBatch::Single(StreamEvent::ToolInputDelta {
                stream_id: None,
                item_id: event
                    .item_id
                    .clone()
                    .or_else(|| event.call_id.clone())
                    .unwrap_or_default(),
                call_id: event.call_id.clone(),
                name: None,
                payload_delta: ToolCallDeltaPayload::CustomInput(
                    event.delta.clone().unwrap_or_default(),
                ),
            }))
        }

        "response.output_item.added" => event
            .item
            .as_ref()
            .and_then(output_item_tool_started)
            .map(StreamEventBatch::Single),

        "response.output_item.done" => event
            .item
            .as_ref()
            .and_then(output_item_tool_completed)
            .map(StreamEventBatch::Many),

        "response.completed" => {
            let usage = event.response.as_ref().and_then(|r| {
                r.get("usage").and_then(|u| {
                    Some(TokenUsage {
                        prompt_tokens: u.get("input_tokens")?.as_u64()?,
                        completion_tokens: u.get("output_tokens")?.as_u64()?,
                        total_tokens: u.get("total_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                        cached_prompt_tokens: u
                            .get("input_tokens_details")
                            .and_then(cached_tokens_from_details)
                            .or_else(|| {
                                u.get("prompt_tokens_details")
                                    .and_then(cached_tokens_from_details)
                            })
                            .or_else(|| u.get("prompt_cache_hit_tokens").and_then(|v| v.as_u64()))
                            .unwrap_or(0),
                    })
                })
            });
            let mut events = Vec::new();
            if let Some(usage) = usage {
                events.push(StreamEvent::Usage(usage));
            }
            events.push(StreamEvent::Completed {
                response_id: event
                    .response
                    .as_ref()
                    .and_then(|r| r.get("id")?.as_str().map(String::from)),
            });
            Some(StreamEventBatch::Many(events))
        }

        "response.failed" => Some(StreamEventBatch::Single(StreamEvent::Failed {
            code: event
                .response
                .as_ref()
                .and_then(|r| r.get("error")?.get("code")?.as_str().map(String::from)),
            message: event
                .response
                .as_ref()
                .and_then(|r| r.get("error")?.get("message")?.as_str().map(String::from))
                .unwrap_or_else(|| "response failed".into()),
        })),

        "response.incomplete" => Some(StreamEventBatch::Single(StreamEvent::Failed {
            code: None,
            message: "response incomplete".into(),
        })),

        _ => None,
    }
}

fn cached_tokens_from_details(details: &serde_json::Value) -> Option<u64> {
    details
        .get("cached_tokens")
        .or_else(|| details.get("cache_read_tokens"))
        .or_else(|| details.get("cached_input_tokens"))
        .and_then(serde_json::Value::as_u64)
}

fn output_item_tool_started(item: &serde_json::Value) -> Option<StreamEvent> {
    let kind = item.get("type")?.as_str()?;
    let item_id = item
        .get("id")
        .and_then(|v| v.as_str())
        .or_else(|| item.get("call_id").and_then(|v| v.as_str()))
        .unwrap_or_default()
        .to_string();
    let call_id = item
        .get("call_id")
        .and_then(|v| v.as_str())
        .map(String::from);
    let name = item.get("name").and_then(|v| v.as_str()).map(String::from);

    match kind {
        "function_call" => Some(StreamEvent::ToolInputStarted {
            stream_id: None,
            item_id,
            call_id,
            name,
            payload_kind: ToolInputPayloadKind::FunctionArguments,
        }),
        "custom_tool_call" => Some(StreamEvent::ToolInputStarted {
            stream_id: None,
            item_id,
            call_id,
            name,
            payload_kind: ToolInputPayloadKind::CustomInput,
        }),
        _ => None,
    }
}

fn output_item_tool_completed(item: &serde_json::Value) -> Option<Vec<StreamEvent>> {
    let kind = item.get("type")?.as_str()?;
    let item_id = item
        .get("id")
        .and_then(|value| value.as_str())
        .or_else(|| item.get("call_id").and_then(|value| value.as_str()))
        .unwrap_or_default()
        .to_string();
    let call_id = item
        .get("call_id")
        .and_then(|value| value.as_str())
        .map(String::from);
    let name = item
        .get("name")
        .and_then(|value| value.as_str())
        .map(String::from);
    match kind {
        "function_call" => {
            let payload = Some(ToolCallDeltaPayload::FunctionArguments(
                value_string(item, "arguments").unwrap_or_default(),
            ));
            Some(vec![
                StreamEvent::ToolInputCompleted {
                    stream_id: None,
                    item_id: item_id.clone(),
                    call_id: call_id.clone(),
                    name: name.clone(),
                    payload: payload.clone(),
                },
                StreamEvent::ToolCallReady {
                    stream_id: None,
                    item_id,
                    call_id,
                    name,
                    payload,
                },
            ])
        }
        "custom_tool_call" => {
            let payload = Some(ToolCallDeltaPayload::CustomInput(
                value_string(item, "input").unwrap_or_default(),
            ));
            Some(vec![
                StreamEvent::ToolInputCompleted {
                    stream_id: None,
                    item_id: item_id.clone(),
                    call_id: call_id.clone(),
                    name: name.clone(),
                    payload: payload.clone(),
                },
                StreamEvent::ToolCallReady {
                    stream_id: None,
                    item_id,
                    call_id,
                    name,
                    payload,
                },
            ])
        }
        _ => None,
    }
}

fn value_string(value: &serde_json::Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn assistant_message_identity(item: Option<&Value>) -> Option<(String, TraceTextChannel)> {
    let item = item?;
    if item.get("type")?.as_str()? != "message" {
        return None;
    }
    if item
        .get("role")
        .and_then(Value::as_str)
        .is_some_and(|role| role != "assistant")
    {
        return None;
    }
    let item_id = item.get("id")?.as_str()?.to_string();
    let channel = match item.get("phase").and_then(Value::as_str) {
        Some("commentary") => TraceTextChannel::Commentary,
        Some("final_answer" | "final") => TraceTextChannel::Final,
        _ => TraceTextChannel::Final,
    };
    Some((item_id, channel))
}

fn assistant_message_text(item: &Value) -> Option<String> {
    let text = item
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|part| part.get("type").and_then(Value::as_str) == Some("output_text"))
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .concat();
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn single_event(event: &SseStreamEvent) -> Option<StreamEvent> {
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
            Some(StreamEvent::ReasoningDelta { delta, .. }) => {
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
            Some(StreamEvent::TextDelta { delta, .. }) => {
                assert_eq!(delta, "9.11 更大。");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn process_chat_reasoning_and_content_from_same_chunk() {
        let event = chat_event(serde_json::json!({
            "reasoning_content": "先比较整数位。",
            "content": "<final>9.11 更大。</final>"
        }));

        match process_sse_events(&event).as_slice() {
            [
                StreamEvent::ReasoningDelta {
                    delta: reasoning, ..
                },
                StreamEvent::TextDelta { delta: content, .. },
            ] => {
                assert_eq!(reasoning, "先比较整数位。");
                assert_eq!(content, "<final>9.11 更大。</final>");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn process_chat_completed_reads_cached_prompt_tokens() {
        let event: SseStreamEvent = serde_json::from_value(serde_json::json!({
            "choices": [{
                "delta": {},
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 20,
                "total_tokens": 120,
                "prompt_tokens_details": {
                    "cached_tokens": 35
                }
            }
        }))
        .unwrap();

        match process_sse_events(&event).as_slice() {
            [
                StreamEvent::Usage(usage),
                StreamEvent::Completed { response_id: None },
            ] => {
                assert_eq!(usage.cached_prompt_tokens, 35);
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn responses_decoder_preserves_native_text_phase_and_completed_text() {
        let mut decoder = OpenAiStreamDecoder::new(true);
        let commentary_added: SseStreamEvent = serde_json::from_value(serde_json::json!({
            "type": "response.output_item.added",
            "item": {
                "id": "msg_progress",
                "type": "message",
                "role": "assistant",
                "phase": "commentary",
                "content": []
            }
        }))
        .unwrap();
        match decoder.decode(&commentary_added).as_slice() {
            [StreamEvent::TextStarted { id, channel }] => {
                assert_eq!(id, "msg_progress");
                assert_eq!(*channel, TraceTextChannel::Commentary);
            }
            other => panic!("unexpected events: {other:?}"),
        }

        let commentary_delta: SseStreamEvent = serde_json::from_value(serde_json::json!({
            "type": "response.output_text.delta",
            "item_id": "msg_progress",
            "delta": "正在检查。"
        }))
        .unwrap();
        match decoder.decode(&commentary_delta).as_slice() {
            [StreamEvent::TextDelta { id, channel, delta }] => {
                assert_eq!(id, "msg_progress");
                assert_eq!(*channel, TraceTextChannel::Commentary);
                assert_eq!(delta, "正在检查。");
            }
            other => panic!("unexpected events: {other:?}"),
        }

        let final_done: SseStreamEvent = serde_json::from_value(serde_json::json!({
            "type": "response.output_item.done",
            "item": {
                "id": "msg_final",
                "type": "message",
                "role": "assistant",
                "phase": "final_answer",
                "content": [
                    {"type": "output_text", "text": "完成。"}
                ]
            }
        }))
        .unwrap();
        match decoder.decode(&final_done).as_slice() {
            [
                StreamEvent::TextCompleted {
                    id,
                    channel,
                    authoritative_text,
                },
            ] => {
                assert_eq!(id, "msg_final");
                assert_eq!(*channel, TraceTextChannel::Final);
                assert_eq!(authoritative_text.as_deref(), Some("完成。"));
            }
            other => panic!("unexpected events: {other:?}"),
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
            Some(StreamEvent::ToolInputDelta {
                item_id,
                call_id,
                payload_delta: ToolCallDeltaPayload::CustomInput(delta),
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
            Some(StreamEvent::ToolInputDelta {
                stream_id,
                item_id,
                name,
                payload_delta: ToolCallDeltaPayload::CustomInput(delta),
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
            Some(StreamEvent::ToolInputDelta {
                stream_id,
                item_id,
                name,
                payload_delta: ToolCallDeltaPayload::FunctionArguments(delta),
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
            Some(StreamEvent::ToolInputStarted {
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
}
