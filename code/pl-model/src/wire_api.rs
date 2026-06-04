#![allow(dead_code)]
use pl_protocol::Result;

use crate::request::{CompletionRequest, CompletionResponse};
use crate::request::{ToolCall, ToolCallKind, ToolCallPayload, ToolFormat, ToolSchema};

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
    DeepSeekChat,
    ZhipuChat,
}

impl WireDispatch {
    pub fn build_request_body(&self, request: &CompletionRequest) -> serde_json::Value {
        match self {
            WireDispatch::Responses => responses_build_body(request),
            WireDispatch::Chat => chat_build_body(request, ChatReasoningStyle::Plain),
            WireDispatch::DeepSeekChat => chat_build_body(request, ChatReasoningStyle::DeepSeek),
            WireDispatch::ZhipuChat => chat_build_body(request, ChatReasoningStyle::Zhipu),
        }
    }

    pub fn parse_response(&self, body: serde_json::Value) -> Result<CompletionResponse> {
        match self {
            WireDispatch::Responses => responses_parse_response(body),
            WireDispatch::Chat | WireDispatch::DeepSeekChat | WireDispatch::ZhipuChat => {
                chat_parse_response(body)
            }
        }
    }

    pub fn parse_stream_event(
        &self,
        event: &crate::sse::SseStreamEvent,
    ) -> Result<Option<crate::sse::StreamEvent>> {
        Ok(crate::sse::process_sse_event(event))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChatReasoningStyle {
    Plain,
    DeepSeek,
    Zhipu,
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

/// 从 metadata 中解析 tool_calls（由 CoreSession::push_assistant_tool_calls 写入）。
fn parse_tool_calls_from_metadata(
    metadata: &std::collections::HashMap<String, String>,
) -> Option<Vec<crate::request::ToolCall>> {
    metadata
        .get("tool_calls")
        .and_then(|json| serde_json::from_str(json).ok())
}

fn tool_result_kind(metadata: &std::collections::HashMap<String, String>) -> ToolCallKind {
    match metadata.get("tool_call_kind").map(String::as_str) {
        Some("custom") => ToolCallKind::Custom,
        Some("function") | None => ToolCallKind::Function,
        Some(_) => ToolCallKind::Function,
    }
}

fn tool_format_json(format: &ToolFormat) -> serde_json::Value {
    match format {
        ToolFormat::Text => serde_json::json!({ "type": "text" }),
        ToolFormat::Grammar { syntax, definition } => serde_json::json!({
            "type": "grammar",
            "syntax": syntax,
            "definition": definition,
        }),
    }
}

fn responses_tool_json(tool: &ToolSchema) -> serde_json::Value {
    match tool {
        ToolSchema::Function {
            name,
            description,
            input_schema,
        } => serde_json::json!({
            "type": "function",
            "name": name,
            "description": description,
            "parameters": input_schema,
        }),
        ToolSchema::Custom {
            name,
            description,
            format,
        } => serde_json::json!({
            "type": "custom",
            "name": name,
            "description": description,
            "format": tool_format_json(format),
        }),
    }
}

fn chat_tool_json(tool: &ToolSchema) -> serde_json::Value {
    match tool {
        ToolSchema::Function {
            name,
            description,
            input_schema,
        } => serde_json::json!({
            "type": "function",
            "function": {
                "name": name,
                "description": description,
                "parameters": input_schema,
            }
        }),
        ToolSchema::Custom {
            name,
            description,
            format,
        } => serde_json::json!({
            "type": "custom",
            "custom": {
                "name": name,
                "description": description,
                "format": tool_format_json(format),
            }
        }),
    }
}

fn cached_tokens_from_usage(usage: &serde_json::Value) -> u64 {
    usage
        .get("prompt_cache_hit_tokens")
        .or_else(|| usage.get("cached_prompt_tokens"))
        .and_then(serde_json::Value::as_u64)
        .or_else(|| {
            usage
                .get("input_tokens_details")
                .and_then(cached_tokens_from_details)
        })
        .or_else(|| {
            usage
                .get("prompt_tokens_details")
                .and_then(cached_tokens_from_details)
        })
        .unwrap_or(0)
}

fn cached_tokens_from_details(details: &serde_json::Value) -> Option<u64> {
    details
        .get("cached_tokens")
        .or_else(|| details.get("cache_read_tokens"))
        .or_else(|| details.get("cached_input_tokens"))
        .and_then(serde_json::Value::as_u64)
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
        match msg.role {
            pl_protocol::MessageRole::Assistant if msg.metadata.contains_key("tool_calls") => {
                // Assistant 消息带 tool_calls
                let text = message_content_text(&msg.content);
                if !text.is_empty() {
                    input.push(serde_json::json!({
                        "type": "message",
                        "role": "assistant",
                        "content": [{"type": "output_text", "text": text}]
                    }));
                }
                if let Some(tool_calls) = parse_tool_calls_from_metadata(&msg.metadata) {
                    for tc in &tool_calls {
                        match &tc.payload {
                            ToolCallPayload::Function { arguments } => {
                                input.push(serde_json::json!({
                                    "type": "function_call",
                                    "id": tc.id,
                                    "name": tc.name,
                                    "arguments": serde_json::to_string(arguments).unwrap_or_default(),
                                    "call_id": tc.call_id.as_deref().unwrap_or(&tc.id),
                                }));
                            }
                            ToolCallPayload::Custom { input: tool_input } => {
                                input.push(serde_json::json!({
                                    "type": "custom_tool_call",
                                    "id": tc.id,
                                    "name": tc.name,
                                    "input": tool_input,
                                    "call_id": tc.call_id.as_deref().unwrap_or(&tc.id),
                                }));
                            }
                        }
                    }
                }
            }
            pl_protocol::MessageRole::Tool => {
                // Tool result 消息
                let call_id = msg
                    .metadata
                    .get("tool_call_id")
                    .cloned()
                    .unwrap_or_default();
                let text = message_content_text(&msg.content);
                match tool_result_kind(&msg.metadata) {
                    ToolCallKind::Function => {
                        input.push(serde_json::json!({
                            "type": "function_call_output",
                            "call_id": call_id,
                            "output": text,
                        }));
                    }
                    ToolCallKind::Custom => {
                        input.push(serde_json::json!({
                            "type": "custom_tool_call_output",
                            "call_id": call_id,
                            "name": msg.metadata.get("tool_name").cloned(),
                            "output": text,
                        }));
                    }
                }
            }
            _ => {
                // 普通消息
                let role = message_role_str(&msg.role);
                let text = message_content_text(&msg.content);
                input.push(serde_json::json!({
                    "type": "message",
                    "role": role,
                    "content": [{"type": "input_text", "text": text}]
                }));
            }
        }
    }

    let tools: Vec<serde_json::Value> = request.tools.iter().map(responses_tool_json).collect();

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
    use crate::request::{FinishReason, TokenUsage};

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
                .filter_map(parse_responses_tool_call)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let usage = body.get("usage").and_then(|u| {
        Some(TokenUsage {
            prompt_tokens: u.get("input_tokens")?.as_u64()?,
            completion_tokens: u.get("output_tokens")?.as_u64()?,
            total_tokens: u.get("total_tokens")?.as_u64().unwrap_or(0),
            cached_prompt_tokens: cached_tokens_from_usage(u),
        })
    });

    let finish_reason = if !tool_calls.is_empty() {
        FinishReason::ToolCalls
    } else {
        FinishReason::Stop
    };

    Ok(CompletionResponse {
        raw_content: content.clone(),
        content,
        reasoning_content: None,
        tool_calls,
        timeline_events: Vec::new(),
        next_sequence: 0,
        usage: usage.unwrap_or_default(),
        finish_reason,
        model: body
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or("")
            .to_string(),
    })
}

fn parse_responses_tool_call(item: &serde_json::Value) -> Option<ToolCall> {
    match item.get("type")?.as_str()? {
        "function_call" => Some(ToolCall::function(
            item.get("id")?.as_str()?.to_string(),
            item.get("name")?.as_str()?.to_string(),
            serde_json::from_str(item.get("arguments")?.as_str()?).ok()?,
            item.get("call_id")
                .and_then(|v| v.as_str())
                .map(String::from),
        )),
        "custom_tool_call" => Some(ToolCall::custom(
            item.get("id")
                .and_then(|v| v.as_str())
                .or_else(|| item.get("call_id").and_then(|v| v.as_str()))?
                .to_string(),
            item.get("name")?.as_str()?.to_string(),
            item.get("input")?.as_str()?.to_string(),
            item.get("call_id")
                .and_then(|v| v.as_str())
                .map(String::from),
        )),
        _ => None,
    }
}

fn chat_build_body(
    request: &CompletionRequest,
    reasoning_style: ChatReasoningStyle,
) -> serde_json::Value {
    let mut messages = Vec::new();

    if let Some(ref instructions) = request.instructions {
        messages.push(serde_json::json!({
            "role": "system",
            "content": instructions
        }));
    }

    for msg in &request.messages {
        match msg.role {
            pl_protocol::MessageRole::Assistant if msg.metadata.contains_key("tool_calls") => {
                // Assistant 消息带 tool_calls
                let text = message_content_text(&msg.content);
                let mut message = serde_json::json!({
                    "role": "assistant",
                    "content": if text.is_empty() { serde_json::Value::Null } else { serde_json::json!(text) }
                });
                if let Some(ref reasoning_content) = msg.reasoning_content {
                    message["reasoning_content"] = serde_json::json!(reasoning_content);
                }
                if let Some(tool_calls) = parse_tool_calls_from_metadata(&msg.metadata) {
                    message["tool_calls"] = serde_json::json!(tool_calls.iter().map(|tc| {
                        match &tc.payload {
                            ToolCallPayload::Function { arguments } => serde_json::json!({
                                "id": tc.id,
                                "type": "function",
                                "function": {
                                    "name": tc.name,
                                    "arguments": serde_json::to_string(arguments).unwrap_or_default()
                                }
                            }),
                            ToolCallPayload::Custom { input } => serde_json::json!({
                                "id": tc.id,
                                "type": "custom",
                                "custom": {
                                    "name": tc.name,
                                    "input": input
                                }
                            }),
                        }
                    }).collect::<Vec<_>>());
                }
                messages.push(message);
            }
            pl_protocol::MessageRole::Tool => {
                // Tool result 消息
                let tool_call_id = msg
                    .metadata
                    .get("tool_call_id")
                    .cloned()
                    .unwrap_or_default();
                let text = message_content_text(&msg.content);
                messages.push(serde_json::json!({
                    "role": "tool",
                    "tool_call_id": tool_call_id,
                    "content": text,
                }));
            }
            _ => {
                // 普通消息
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
        }
    }

    let tools: Vec<serde_json::Value> = request.tools.iter().map(chat_tool_json).collect();

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
        match reasoning_style {
            ChatReasoningStyle::Plain => {}
            ChatReasoningStyle::DeepSeek => {
                if let Some(ref effort) = reasoning.effort {
                    body["reasoning_effort"] = serde_json::json!(effort);
                }
                body["thinking"] = serde_json::json!({
                    "type": chat_thinking_type(reasoning)
                });
            }
            ChatReasoningStyle::Zhipu => {
                let thinking_type = chat_thinking_type(reasoning);
                let mut thinking = serde_json::json!({ "type": thinking_type });
                if thinking_type == "enabled" {
                    thinking["clear_thinking"] = serde_json::json!(false);
                }
                body["thinking"] = thinking;
            }
        }
    }

    body
}

fn chat_thinking_type(reasoning: &crate::request::ReasoningConfig) -> &'static str {
    match reasoning.summary {
        Some(crate::request::ReasoningSummary::Disabled) => "disabled",
        Some(crate::request::ReasoningSummary::Auto)
        | Some(crate::request::ReasoningSummary::Enabled)
        | None => "enabled",
    }
}

fn chat_parse_response(body: serde_json::Value) -> Result<CompletionResponse> {
    use crate::request::{FinishReason, TokenUsage};

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
                .filter_map(parse_chat_tool_call)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let usage = body.get("usage").and_then(|u| {
        Some(TokenUsage {
            prompt_tokens: u.get("prompt_tokens")?.as_u64()?,
            completion_tokens: u.get("completion_tokens")?.as_u64()?,
            total_tokens: u.get("total_tokens")?.as_u64()?,
            cached_prompt_tokens: cached_tokens_from_usage(u),
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
        raw_content: content.clone(),
        content,
        reasoning_content,
        tool_calls,
        timeline_events: Vec::new(),
        next_sequence: 0,
        usage: usage.unwrap_or_default(),
        finish_reason,
        model: body
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or("")
            .to_string(),
    })
}

fn parse_chat_tool_call(item: &serde_json::Value) -> Option<ToolCall> {
    match item.get("type").and_then(|value| value.as_str()) {
        Some("custom") => {
            let custom = item.get("custom")?;
            Some(ToolCall::custom(
                item.get("id")?.as_str()?.to_string(),
                custom.get("name")?.as_str()?.to_string(),
                custom.get("input")?.as_str()?.to_string(),
                None,
            ))
        }
        Some("function") | None => {
            let func = item.get("function")?;
            Some(ToolCall::function(
                item.get("id")?.as_str()?.to_string(),
                func.get("name")?.as_str()?.to_string(),
                serde_json::from_str(func.get("arguments")?.as_str()?).ok()?,
                None,
            ))
        }
        Some(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use pl_protocol::{Message, MessageContent, MessageRole};
    use pretty_assertions::assert_eq;

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
            timeline: None,
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
    fn generic_chat_body_omits_provider_specific_reasoning_fields() {
        let body = WireDispatch::Chat.build_request_body(&request_with_effort("max"));

        assert!(body.get("reasoning_effort").is_none());
        assert!(body.get("thinking").is_none());
    }

    #[test]
    fn deepseek_chat_body_writes_thinking_mode() {
        let body = WireDispatch::DeepSeekChat.build_request_body(&request_with_effort("max"));

        assert_eq!(body["reasoning_effort"], serde_json::json!("max"));
        assert_eq!(body["thinking"]["type"], serde_json::json!("enabled"));
    }

    #[test]
    fn deepseek_chat_body_writes_disabled_thinking_mode() {
        let mut request = request_with_effort("high");
        request.reasoning.as_mut().unwrap().summary = Some(ReasoningSummary::Disabled);

        let body = WireDispatch::DeepSeekChat.build_request_body(&request);

        assert_eq!(body["thinking"]["type"], serde_json::json!("disabled"));
    }

    #[test]
    fn zhipu_chat_body_writes_official_thinking_mode() {
        let body = WireDispatch::ZhipuChat.build_request_body(&request_with_effort("enabled"));

        assert!(body.get("reasoning_effort").is_none());
        assert_eq!(body["thinking"]["type"], serde_json::json!("enabled"));
        assert_eq!(body["thinking"]["clear_thinking"], serde_json::json!(false));
    }

    #[test]
    fn zhipu_chat_body_writes_disabled_thinking_mode() {
        let mut request = request_with_effort("none");
        request.reasoning.as_mut().unwrap().summary = Some(ReasoningSummary::Disabled);

        let body = WireDispatch::ZhipuChat.build_request_body(&request);

        assert!(body.get("reasoning_effort").is_none());
        assert_eq!(body["thinking"]["type"], serde_json::json!("disabled"));
        assert!(body["thinking"].get("clear_thinking").is_none());
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

    #[test]
    fn chat_parse_response_reads_cached_prompt_tokens() {
        let response = WireDispatch::Chat
            .parse_response(serde_json::json!({
                "model": "deepseek-v4-flash",
                "choices": [{
                    "message": {
                        "role": "assistant",
                        "content": "ok"
                    },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 100,
                    "completion_tokens": 20,
                    "total_tokens": 120,
                    "prompt_tokens_details": {
                        "cached_tokens": 40
                    }
                }
            }))
            .unwrap();

        assert_eq!(response.usage.cached_prompt_tokens, 40);
    }

    #[test]
    fn responses_parse_response_reads_cached_input_tokens() {
        let response = WireDispatch::Responses
            .parse_response(serde_json::json!({
                "model": "gpt-5.5",
                "output": [{
                    "type": "message",
                    "content": [{ "text": "ok" }]
                }],
                "usage": {
                    "input_tokens": 100,
                    "output_tokens": 20,
                    "total_tokens": 120,
                    "input_tokens_details": {
                        "cached_tokens": 55
                    }
                }
            }))
            .unwrap();

        assert_eq!(response.usage.cached_prompt_tokens, 55);
    }

    #[test]
    fn responses_body_writes_custom_grammar_tool() {
        let mut request = request_with_effort("xhigh");
        request.tools = vec![ToolSchema::custom_grammar(
            "apply_patch",
            "edit files",
            "lark",
            "start: patch",
        )];

        let body = WireDispatch::Responses.build_request_body(&request);

        assert_eq!(body["tools"][0]["type"], serde_json::json!("custom"));
        assert_eq!(body["tools"][0]["name"], serde_json::json!("apply_patch"));
        assert_eq!(
            body["tools"][0]["format"],
            serde_json::json!({
                "type": "grammar",
                "syntax": "lark",
                "definition": "start: patch"
            })
        );
    }

    #[test]
    fn chat_body_writes_custom_grammar_tool() {
        let mut request = request_with_effort("xhigh");
        request.tools = vec![ToolSchema::custom_grammar(
            "apply_patch",
            "edit files",
            "lark",
            "start: patch",
        )];

        let body = WireDispatch::Chat.build_request_body(&request);

        assert_eq!(body["tools"][0]["type"], serde_json::json!("custom"));
        assert_eq!(
            body["tools"][0]["custom"]["name"],
            serde_json::json!("apply_patch")
        );
    }

    #[test]
    fn provider_compatible_turns_custom_apply_patch_into_function_fallback() {
        let mut request = request_with_effort("high");
        request.tools = vec![ToolSchema::custom_grammar(
            "apply_patch",
            "edit files",
            "lark",
            "start: patch",
        )];

        let request = request.provider_compatible(false);
        let body = WireDispatch::Chat.build_request_body(&request);

        assert_eq!(body["tools"][0]["type"], serde_json::json!("function"));
        assert_eq!(
            body["tools"][0]["function"]["parameters"]["required"],
            serde_json::json!(["patch"])
        );
        let description =
            body["tools"][0]["function"]["parameters"]["properties"]["patch"]["description"]
                .as_str()
                .unwrap();
        assert!(description.contains("*** Add File:"));
        assert!(description.contains("*** Update File:"));
        assert!(description.contains("---/+++ unified diff"));
        assert!(description.contains("Minimal update example:"));
        assert!(description.contains("*** Update File: notes.txt"));
        assert!(description.contains("-old line"));
        assert!(description.contains("+new line"));
    }

    #[test]
    fn responses_parse_response_reads_custom_tool_call() {
        let response = WireDispatch::Responses
            .parse_response(serde_json::json!({
                "model": "gpt-5.5",
                "output": [{
                    "type": "custom_tool_call",
                    "id": "ctc_1",
                    "call_id": "call_1",
                    "name": "apply_patch",
                    "input": "*** Begin Patch\n*** End Patch"
                }],
                "usage": {
                    "input_tokens": 1,
                    "output_tokens": 1,
                    "total_tokens": 2
                }
            }))
            .unwrap();

        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].name, "apply_patch");
        match &response.tool_calls[0].payload {
            ToolCallPayload::Custom { input } => {
                assert_eq!(input, "*** Begin Patch\n*** End Patch");
            }
            other => panic!("unexpected payload: {other:?}"),
        }
    }

    #[test]
    fn chat_parse_response_reads_custom_tool_call() {
        let response = WireDispatch::Chat
            .parse_response(serde_json::json!({
                "model": "gpt-5.5",
                "choices": [{
                    "message": {
                        "role": "assistant",
                        "tool_calls": [{
                            "id": "call_1",
                            "type": "custom",
                            "custom": {
                                "name": "apply_patch",
                                "input": "*** Begin Patch\n*** End Patch"
                            }
                        }]
                    },
                    "finish_reason": "tool_calls"
                }],
                "usage": {
                    "prompt_tokens": 1,
                    "completion_tokens": 1,
                    "total_tokens": 2
                }
            }))
            .unwrap();

        assert_eq!(response.tool_calls.len(), 1);
        assert!(matches!(
            response.tool_calls[0].payload,
            ToolCallPayload::Custom { .. }
        ));
    }

    #[test]
    fn responses_history_replays_custom_tool_call_and_output() {
        let mut metadata = HashMap::new();
        let calls = vec![ToolCall::custom(
            "ctc_1",
            "apply_patch",
            "*** Begin Patch\n*** End Patch",
            Some("call_1".to_string()),
        )];
        metadata.insert(
            "tool_calls".to_string(),
            serde_json::to_string(&calls).unwrap(),
        );
        let mut tool_metadata = HashMap::new();
        tool_metadata.insert("tool_call_id".to_string(), "call_1".to_string());
        tool_metadata.insert("tool_call_kind".to_string(), "custom".to_string());
        tool_metadata.insert("tool_name".to_string(), "apply_patch".to_string());
        let request = CompletionRequest {
            model: "gpt-5.5".to_string(),
            instructions: None,
            messages: vec![
                Message {
                    role: MessageRole::Assistant,
                    content: MessageContent::Text(String::new()),
                    reasoning_content: None,
                    metadata,
                },
                Message {
                    role: MessageRole::Tool,
                    content: MessageContent::Text("ok".to_string()),
                    reasoning_content: None,
                    metadata: tool_metadata,
                },
            ],
            tools: Vec::new(),
            tool_choice: "auto".to_string(),
            parallel_tool_calls: false,
            temperature: None,
            max_tokens: None,
            reasoning: None,
            stream: true,
            timeline: None,
        };

        let body = WireDispatch::Responses.build_request_body(&request);

        assert_eq!(
            body["input"][0]["type"],
            serde_json::json!("custom_tool_call")
        );
        assert_eq!(
            body["input"][1]["type"],
            serde_json::json!("custom_tool_call_output")
        );
    }
}
