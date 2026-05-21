#![allow(dead_code)]
use pl_protocol::Result;

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

fn message_role_str(role: &pl_protocol::MessageRole) -> &str {
    match role {
        pl_protocol::MessageRole::System => "system",
        pl_protocol::MessageRole::User => "user",
        pl_protocol::MessageRole::Assistant => "assistant",
        pl_protocol::MessageRole::Tool => "tool",
    }
}

fn message_content_text(content: &pl_protocol::MessageContent) -> String {
    match content {
        pl_protocol::MessageContent::Text(t) => t.clone(),
        pl_protocol::MessageContent::MultiPart(parts) => parts
            .iter()
            .map(|p| p.text.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
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
        let role = message_role_str(&msg.role);
        let text = message_content_text(&msg.content);
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
        if let Some(ref effort) = reasoning.effort {
            r["effort"] = serde_json::json!(effort);
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
        reasoning_content: None,
        tool_calls,
        usage: usage.unwrap_or_default(),
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
        let role = message_role_str(&msg.role);
        let text = message_content_text(&msg.content);
        let mut message = serde_json::json!({
            "role": role,
            "content": text
        });

        if msg.role == pl_protocol::MessageRole::Assistant
            && let Some(reasoning_content) = &msg.reasoning_content
        {
            message["reasoning_content"] = serde_json::json!(reasoning_content);
        }

        messages.push(message);
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

    if let Some(ref reasoning) = request.reasoning {
        if let Some(ref effort) = reasoning.effort {
            body["reasoning_effort"] = serde_json::json!(effort);
        }

        body["thinking"] = serde_json::json!({
            "type": match reasoning.summary {
                Some(crate::request::ReasoningSummary::Disabled) => "disabled",
                Some(crate::request::ReasoningSummary::Auto)
                | Some(crate::request::ReasoningSummary::Enabled)
                | None => "enabled",
            }
        });
    }

    body
}

fn chat_parse_response(body: serde_json::Value) -> Result<CompletionResponse> {
    use crate::request::{FinishReason, TokenUsage, ToolCall};

    let choice = body
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first());

    let message = choice.and_then(|c| c.get("message"));

    let content = message
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .map(String::from);

    let reasoning_content = message
        .and_then(|m| m.get("reasoning_content"))
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
        reasoning_content,
        tool_calls,
        usage: usage.unwrap_or_default(),
        finish_reason,
        model: body
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or("")
            .to_string(),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use pl_protocol::{Message, MessageContent, MessageRole};

    use super::*;
    use crate::request::{ReasoningConfig, ReasoningSummary};

    fn text_message(role: MessageRole, content: &str) -> Message {
        Message {
            role,
            content: MessageContent::Text(content.to_string()),
            reasoning_content: None,
            metadata: HashMap::new(),
        }
    }

    fn request_with_effort(effort: &str) -> CompletionRequest {
        CompletionRequest {
            model: "gpt-5.5".to_string(),
            instructions: None,
            messages: vec![text_message(MessageRole::User, "hello")],
            tools: Vec::new(),
            tool_choice: "auto".to_string(),
            parallel_tool_calls: true,
            temperature: None,
            max_tokens: None,
            reasoning: Some(ReasoningConfig {
                effort: Some(effort.to_string()),
                summary: None,
            }),
            stream: true,
        }
    }

    #[test]
    fn responses_body_writes_xhigh_reasoning_effort() {
        let body = WireDispatch::Responses.build_request_body(&request_with_effort("xhigh"));

        assert_eq!(body["reasoning"]["effort"], serde_json::json!("xhigh"));
    }

    #[test]
    fn responses_body_accepts_custom_reasoning_effort() {
        let body =
            WireDispatch::Responses.build_request_body(&request_with_effort("custom-effort"));

        assert_eq!(
            body["reasoning"]["effort"],
            serde_json::json!("custom-effort")
        );
    }

    #[test]
    fn chat_body_writes_deepseek_thinking_mode() {
        let body = WireDispatch::Chat.build_request_body(&request_with_effort("max"));

        assert_eq!(body["reasoning_effort"], serde_json::json!("max"));
        assert_eq!(body["thinking"]["type"], serde_json::json!("enabled"));
    }

    #[test]
    fn chat_body_writes_disabled_thinking_mode() {
        let mut request = request_with_effort("high");
        request.reasoning.as_mut().unwrap().summary = Some(ReasoningSummary::Disabled);

        let body = WireDispatch::Chat.build_request_body(&request);

        assert_eq!(body["thinking"]["type"], serde_json::json!("disabled"));
    }

    #[test]
    fn chat_body_writes_assistant_reasoning_content() {
        let mut request = request_with_effort("high");
        request.messages = vec![Message {
            role: MessageRole::Assistant,
            content: MessageContent::Text("9.11 更大。".to_string()),
            reasoning_content: Some("比较小数位。".to_string()),
            metadata: HashMap::new(),
        }];

        let body = WireDispatch::Chat.build_request_body(&request);

        assert_eq!(
            body["messages"][0]["reasoning_content"],
            serde_json::json!("比较小数位。")
        );
    }

    #[test]
    fn chat_parse_response_reads_reasoning_content() {
        let response = WireDispatch::Chat
            .parse_response(serde_json::json!({
                "model": "deepseek-v4-flash",
                "choices": [{
                    "message": {
                        "role": "assistant",
                        "reasoning_content": "先比较整数，再比较小数。",
                        "content": "9.11 更大。"
                    },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 4,
                    "completion_tokens": 8,
                    "total_tokens": 12
                }
            }))
            .unwrap();

        assert_eq!(response.content.as_deref(), Some("9.11 更大。"));
        assert_eq!(
            response.reasoning_content.as_deref(),
            Some("先比较整数，再比较小数。")
        );
    }
}
