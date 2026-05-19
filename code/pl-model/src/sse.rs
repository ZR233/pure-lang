use serde::Deserialize;

use crate::request::TokenUsage;

/// SSE 流事件原始结构（从 JSON 解析）
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct SseStreamEvent {
    #[serde(rename = "type")]
    pub kind: String,
    pub delta: Option<String>,
    pub item: Option<serde_json::Value>,
    pub item_id: Option<String>,
    pub call_id: Option<String>,
    pub response: Option<serde_json::Value>,
    pub summary_index: Option<i64>,
    pub content_index: Option<i64>,
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
        delta: String,
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

/// 将原始 SSE 事件解析为结构化事件
pub fn process_sse_event(event: &SseStreamEvent) -> Option<StreamEvent> {
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
            delta: event.delta.clone().unwrap_or_default(),
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
