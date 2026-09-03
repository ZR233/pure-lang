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
    let choice = event.choices.as_ref().and_then(|choices| choices.first())?;
    let mut events = Vec::new();
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

    if choice.finish_reason.is_some() {
        let usage = event
            .usage
            .as_ref()
            .and_then(ProviderTokenUsage::to_chat_usage);
        if let Some(usage) = usage {
            events.push(ModelStreamEvent::Usage(usage));
        }
        events.push(ModelStreamEvent::Completed { response_id: None });
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
            Some(StreamEventBatch::Single(provider_failure_event(
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
            )))
        }

        "response.incomplete" => {
            let response = event.response.as_ref();
            let reason = response
                .and_then(|response| response.get("incomplete_details")?.get("reason")?.as_str());
            Some(StreamEventBatch::Single(provider_failure_event(
                response.and_then(|response| response.get("error")),
                reason,
                None,
                response.and_then(provider_status),
                None,
                "response incomplete",
            )))
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
mod unit_tests;
