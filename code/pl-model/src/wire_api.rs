#![allow(dead_code)]
use pl_core::Result;

use crate::request::{CompletionRequest, CompletionResponse};

/// API 协议适配器。
///
/// 将内部统一的 CompletionRequest 转换为不同 provider 的 wire 格式，
/// 并将 provider 返回的响应解析回 CompletionResponse。
///
/// 实现者契约：
/// - build_request_body() 产生的 JSON 必须符合目标 API 规范
/// - parse_stream_event() 处理单个 SSE 事件，返回 None 表示跳过
pub trait WireAdapter: Send + Sync {
    fn build_request_body(&self, request: &CompletionRequest) -> serde_json::Value;
    fn parse_response(&self, body: serde_json::Value) -> Result<CompletionResponse>;
    fn parse_stream_event(
        &self,
        event: &crate::sse::SseStreamEvent,
    ) -> Result<Option<crate::sse::StreamEvent>>;
}

/// Wire 协议分发：Responses API vs Chat Completions API
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireDispatch {
    Responses,
    Chat,
}

impl WireDispatch {
    pub fn build_request_body(&self, request: &CompletionRequest) -> serde_json::Value {
        match self {
            WireDispatch::Responses => responses_build_body(request),
            WireDispatch::Chat => chat_build_body(request),
        }
    }

    pub fn parse_response(&self, body: serde_json::Value) -> Result<CompletionResponse> {
        match self {
            WireDispatch::Responses => responses_parse_response(body),
            WireDispatch::Chat => chat_parse_response(body),
        }
    }

    pub fn parse_stream_event(
        &self,
        event: &crate::sse::SseStreamEvent,
    ) -> Result<Option<crate::sse::StreamEvent>> {
        Ok(crate::sse::process_sse_event(event))
    }
}

fn responses_build_body(request: &CompletionRequest) -> serde_json::Value {
    let mut input = Vec::new();

    if let Some(ref instructions) = request.instructions {
        input.push(serde_json::json!({
            "type": "message",
            "role": "system",
            "content": [{"type": "input_text", "text": instructions}]
        }));
    }

    for msg in &request.messages {
        let role = match msg.role {
            pl_core::MessageRole::System => "system",
            pl_core::MessageRole::User => "user",
            pl_core::MessageRole::Assistant => "assistant",
            pl_core::MessageRole::Tool => "tool",
        };
        let text = match &msg.content {
            pl_core::MessageContent::Text(t) => t.clone(),
            pl_core::MessageContent::MultiPart(parts) => parts
                .iter()
                .map(|p| p.text.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
        };
        input.push(serde_json::json!({
            "type": "message",
            "role": role,
            "content": [{"type": "input_text", "text": text}]
        }));
    }

    let tools: Vec<serde_json::Value> = request
        .tools
        .iter()
        .map(|t| {
            serde_json::json!({
                "type": "function",
                "name": t.name,
                "description": t.description,
                "parameters": t.input_schema,
            })
        })
        .collect();

    let mut body = serde_json::json!({
        "model": request.model,
        "input": input,
        "stream": true,
        "tool_choice": request.tool_choice,
        "parallel_tool_calls": request.parallel_tool_calls,
    });

    if !tools.is_empty() {
        body["tools"] = serde_json::json!(tools);
    }

    if let Some(temp) = request.temperature {
        body["temperature"] = serde_json::json!(temp);
    }

    if let Some(max_tokens) = request.max_tokens {
        body["max_output_tokens"] = serde_json::json!(max_tokens);
    }

    if let Some(ref reasoning) = request.reasoning {
        let mut r = serde_json::json!({});
        if let Some(effort) = reasoning.effort {
            r["effort"] = serde_json::json!(match effort {
                crate::request::ReasoningEffort::Low => "low",
                crate::request::ReasoningEffort::Medium => "medium",
                crate::request::ReasoningEffort::High => "high",
            });
        }
        if let Some(summary) = reasoning.summary {
            r["summary"] = serde_json::json!(match summary {
                crate::request::ReasoningSummary::Auto => "auto",
                crate::request::ReasoningSummary::Enabled => "enabled",
                crate::request::ReasoningSummary::Disabled => "disabled",
            });
        }
        body["reasoning"] = r;
    }

    body
}

fn responses_parse_response(body: serde_json::Value) -> Result<CompletionResponse> {
    use crate::request::{FinishReason, TokenUsage, ToolCall};

    let content = body
        .get("output")
        .and_then(|o| o.as_array())
        .and_then(|items| {
            items.iter().find_map(|item| {
                if item.get("type")?.as_str()? == "message" {
                    item.get("content")?
                        .as_array()?
                        .first()?
                        .get("text")?
                        .as_str()
                        .map(String::from)
                } else {
                    None
                }
            })
        });

    let tool_calls = body
        .get("output")
        .and_then(|o| o.as_array())
        .map(|items| {
            items
                .iter()
                .filter(|item| item.get("type").and_then(|t| t.as_str()) == Some("function_call"))
                .filter_map(|item| {
                    Some(ToolCall {
                        id: item.get("id")?.as_str()?.to_string(),
                        name: item.get("name")?.as_str()?.to_string(),
                        arguments: serde_json::from_str(item.get("arguments")?.as_str()?).ok()?,
                        call_id: item
                            .get("call_id")
                            .and_then(|v| v.as_str())
                            .map(String::from),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let usage = body.get("usage").and_then(|u| {
        Some(TokenUsage {
            prompt_tokens: u.get("input_tokens")?.as_u64()?,
            completion_tokens: u.get("output_tokens")?.as_u64()?,
            total_tokens: u.get("total_tokens")?.as_u64().unwrap_or(0),
        })
    });

    let finish_reason = if !tool_calls.is_empty() {
        FinishReason::ToolCalls
    } else {
        FinishReason::Stop
    };

    Ok(CompletionResponse {
        content,
        tool_calls,
        usage: usage.unwrap_or(TokenUsage {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
        }),
        finish_reason,
        model: body
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or("")
            .to_string(),
    })
}

fn chat_build_body(request: &CompletionRequest) -> serde_json::Value {
    let mut messages = Vec::new();

    if let Some(ref instructions) = request.instructions {
        messages.push(serde_json::json!({
            "role": "system",
            "content": instructions
        }));
    }

    for msg in &request.messages {
        let role = match msg.role {
            pl_core::MessageRole::System => "system",
            pl_core::MessageRole::User => "user",
            pl_core::MessageRole::Assistant => "assistant",
            pl_core::MessageRole::Tool => "tool",
        };
        let text = match &msg.content {
            pl_core::MessageContent::Text(t) => t.clone(),
            pl_core::MessageContent::MultiPart(parts) => parts
                .iter()
                .map(|p| p.text.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
        };
        messages.push(serde_json::json!({
            "role": role,
            "content": text
        }));
    }

    let tools: Vec<serde_json::Value> = request
        .tools
        .iter()
        .map(|t| {
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.input_schema,
                }
            })
        })
        .collect();

    let mut body = serde_json::json!({
        "model": request.model,
        "messages": messages,
        "stream": true,
    });

    if !tools.is_empty() {
        body["tools"] = serde_json::json!(tools);
        body["tool_choice"] = serde_json::json!(request.tool_choice);
    }

    if let Some(temp) = request.temperature {
        body["temperature"] = serde_json::json!(temp);
    }

    if let Some(max_tokens) = request.max_tokens {
        body["max_tokens"] = serde_json::json!(max_tokens);
    }

    body
}

fn chat_parse_response(body: serde_json::Value) -> Result<CompletionResponse> {
    use crate::request::{FinishReason, TokenUsage, ToolCall};

    let choice = body
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first());

    let content = choice
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .map(String::from);

    let tool_calls = choice
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("tool_calls"))
        .and_then(|tc| tc.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    let func = item.get("function")?;
                    Some(ToolCall {
                        id: item.get("id")?.as_str()?.to_string(),
                        name: func.get("name")?.as_str()?.to_string(),
                        arguments: serde_json::from_str(func.get("arguments")?.as_str()?).ok()?,
                        call_id: None,
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let usage = body.get("usage").and_then(|u| {
        Some(TokenUsage {
            prompt_tokens: u.get("prompt_tokens")?.as_u64()?,
            completion_tokens: u.get("completion_tokens")?.as_u64()?,
            total_tokens: u.get("total_tokens")?.as_u64()?,
        })
    });

    let finish_reason = choice
        .and_then(|c| c.get("finish_reason"))
        .and_then(|r| r.as_str())
        .map(|r| match r {
            "tool_calls" => FinishReason::ToolCalls,
            "length" => FinishReason::MaxTokens,
            "content_filter" => FinishReason::ContentFilter,
            _ => FinishReason::Stop,
        })
        .unwrap_or(FinishReason::Stop);

    Ok(CompletionResponse {
        content,
        tool_calls,
        usage: usage.unwrap_or(TokenUsage {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
        }),
        finish_reason,
        model: body
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or("")
            .to_string(),
    })
}
