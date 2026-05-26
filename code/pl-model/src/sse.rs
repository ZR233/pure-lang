use serde::Deserialize;

use crate::request::TokenUsage;

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
}

/// 解析后的结构化流事件
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum StreamEvent {
    Created,

    OutputTextDelta(String),

    ThinkingDelta {
        delta: String,
    },

    ToolCallDelta {
        item_id: String,
        call_id: Option<String>,
        name: Option<String>,
        payload_delta: ToolCallDeltaPayload,
    },

    OutputItemDone(serde_json::Value),

    Completed {
        response_id: Option<String>,
        usage: Option<TokenUsage>,
    },

    Failed {
        code: Option<String>,
        message: String,
    },
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum ToolCallDeltaPayload {
    FunctionArguments(String),
    CustomInput(String),
}

impl ToolCallDeltaPayload {
    pub fn text(&self) -> &str {
        match self {
            Self::FunctionArguments(delta) | Self::CustomInput(delta) => delta,
        }
    }
}

/// 将原始 SSE 事件解析为结构化事件
pub fn process_sse_event(event: &SseStreamEvent) -> Option<StreamEvent> {
    if let Some(choice) = event.choices.as_ref().and_then(|choices| choices.first()) {
        if let Some(delta) = &choice.delta.reasoning_content
            && !delta.is_empty()
        {
            return Some(StreamEvent::ThinkingDelta {
                delta: delta.clone(),
            });
        }

        if let Some(content) = &choice.delta.content
            && !content.is_empty()
        {
            return Some(StreamEvent::OutputTextDelta(content.clone()));
        }

        if let Some(tool_call) = choice
            .delta
            .tool_calls
            .as_ref()
            .and_then(|tool_calls| tool_calls.first())
        {
            let item_id = tool_call
                .id
                .clone()
                .unwrap_or_else(|| tool_call.index.unwrap_or_default().to_string());
            if let Some(custom) = &tool_call.custom {
                return Some(StreamEvent::ToolCallDelta {
                    item_id,
                    call_id: None,
                    name: custom.name.clone(),
                    payload_delta: ToolCallDeltaPayload::CustomInput(
                        custom.input.clone().unwrap_or_default(),
                    ),
                });
            }
            if let Some(function) = &tool_call.function {
                return Some(StreamEvent::ToolCallDelta {
                    item_id,
                    call_id: None,
                    name: function.name.clone(),
                    payload_delta: ToolCallDeltaPayload::FunctionArguments(
                        function.arguments.clone().unwrap_or_default(),
                    ),
                });
            }
        }

        if choice.finish_reason.is_some() {
            let usage = event.usage.as_ref().map(|u| TokenUsage {
                prompt_tokens: u.prompt_tokens.unwrap_or(0),
                completion_tokens: u.completion_tokens.unwrap_or(0),
                total_tokens: u.total_tokens.unwrap_or(0),
            });
            return Some(StreamEvent::Completed {
                response_id: None,
                usage,
            });
        }
    }

    match event.kind.as_str() {
        "response.created" => Some(StreamEvent::Created),

        "response.output_text.delta" => event
            .delta
            .as_ref()
            .map(|d| StreamEvent::OutputTextDelta(d.clone())),

        "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => event
            .delta
            .as_ref()
            .map(|d| StreamEvent::ThinkingDelta { delta: d.clone() }),

        "response.function_call_arguments.delta" => Some(StreamEvent::ToolCallDelta {
            item_id: event.item_id.clone().unwrap_or_default(),
            call_id: event.call_id.clone(),
            name: None,
            payload_delta: ToolCallDeltaPayload::FunctionArguments(
                event.delta.clone().unwrap_or_default(),
            ),
        }),

        "response.custom_tool_call_input.delta" => Some(StreamEvent::ToolCallDelta {
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
        }),

        "response.output_item.done" => event
            .item
            .as_ref()
            .map(|v| StreamEvent::OutputItemDone(v.clone())),

        "response.completed" => {
            let usage = event.response.as_ref().and_then(|r| {
                r.get("usage").and_then(|u| {
                    Some(TokenUsage {
                        prompt_tokens: u.get("input_tokens")?.as_u64()?,
                        completion_tokens: u.get("output_tokens")?.as_u64()?,
                        total_tokens: u.get("total_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                    })
                })
            });
            Some(StreamEvent::Completed {
                response_id: event
                    .response
                    .as_ref()
                    .and_then(|r| r.get("id")?.as_str().map(String::from)),
                usage,
            })
        }

        "response.failed" => Some(StreamEvent::Failed {
            code: event
                .response
                .as_ref()
                .and_then(|r| r.get("error")?.get("code")?.as_str().map(String::from)),
            message: event
                .response
                .as_ref()
                .and_then(|r| r.get("error")?.get("message")?.as_str().map(String::from))
                .unwrap_or_else(|| "response failed".into()),
        }),

        "response.incomplete" => Some(StreamEvent::Failed {
            code: None,
            message: "response incomplete".into(),
        }),

        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

        match process_sse_event(&event) {
            Some(StreamEvent::ThinkingDelta { delta }) => {
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

        match process_sse_event(&event) {
            Some(StreamEvent::OutputTextDelta(delta)) => {
                assert_eq!(delta, "9.11 更大。");
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

        match process_sse_event(&event) {
            Some(StreamEvent::ToolCallDelta {
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

        match process_sse_event(&event) {
            Some(StreamEvent::ToolCallDelta {
                item_id,
                name,
                payload_delta: ToolCallDeltaPayload::CustomInput(delta),
                ..
            }) => {
                assert_eq!(item_id, "call_1");
                assert_eq!(name.as_deref(), Some("apply_patch"));
                assert_eq!(delta, "*** Begin Patch\n");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }
}
