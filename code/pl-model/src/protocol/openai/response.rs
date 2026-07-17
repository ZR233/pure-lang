use pl_protocol::{PureError, Result};
use serde::Deserialize;

use crate::request::{CompletionResponse, FinishReason, TokenUsage, ToolCall};
use crate::tool_arguments::function_tool_call_from_raw;

#[derive(Debug, Clone, Deserialize)]
struct ProviderTokenUsage {
    prompt_tokens: Option<u64>,
    input_tokens: Option<u64>,
    completion_tokens: Option<u64>,
    output_tokens: Option<u64>,
    total_tokens: Option<u64>,
    prompt_cache_hit_tokens: Option<u64>,
    cached_prompt_tokens: Option<u64>,
    prompt_tokens_details: Option<TokenUsageDetails>,
    input_tokens_details: Option<TokenUsageDetails>,
    completion_tokens_details: Option<TokenUsageDetails>,
    output_tokens_details: Option<TokenUsageDetails>,
}

impl ProviderTokenUsage {
    fn cached_prompt_tokens(&self) -> u64 {
        self.prompt_cache_hit_tokens
            .or(self.cached_prompt_tokens)
            .or_else(|| {
                self.input_tokens_details
                    .as_ref()
                    .and_then(TokenUsageDetails::cached)
            })
            .or_else(|| {
                self.prompt_tokens_details
                    .as_ref()
                    .and_then(TokenUsageDetails::cached)
            })
            .unwrap_or(0)
    }

    fn reasoning_tokens(&self) -> u64 {
        self.output_tokens_details
            .as_ref()
            .and_then(TokenUsageDetails::reasoning)
            .or_else(|| {
                self.completion_tokens_details
                    .as_ref()
                    .and_then(TokenUsageDetails::reasoning)
            })
            .unwrap_or(0)
    }

    fn to_responses_usage(&self) -> Option<TokenUsage> {
        Some(TokenUsage {
            prompt_tokens: self.input_tokens?,
            completion_tokens: self.output_tokens?,
            total_tokens: self.total_tokens.unwrap_or(0),
            cached_prompt_tokens: self.cached_prompt_tokens(),
            reasoning_tokens: self.reasoning_tokens(),
        })
    }

    fn to_chat_usage(&self) -> Option<TokenUsage> {
        Some(TokenUsage {
            prompt_tokens: self.prompt_tokens?,
            completion_tokens: self.completion_tokens?,
            total_tokens: self.total_tokens.unwrap_or(0),
            cached_prompt_tokens: self.cached_prompt_tokens(),
            reasoning_tokens: self.reasoning_tokens(),
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
struct TokenUsageDetails {
    cached_tokens: Option<u64>,
    cache_read_tokens: Option<u64>,
    cached_input_tokens: Option<u64>,
    reasoning_tokens: Option<u64>,
}

impl TokenUsageDetails {
    fn cached(&self) -> Option<u64> {
        self.cached_tokens
            .or(self.cache_read_tokens)
            .or(self.cached_input_tokens)
    }

    fn reasoning(&self) -> Option<u64> {
        self.reasoning_tokens
    }
}

#[derive(Debug, Clone, Deserialize)]
struct ResponsesResponseBody {
    id: Option<String>,
    model: Option<String>,
    output: Option<Vec<ResponsesOutputItem>>,
    usage: Option<ProviderTokenUsage>,
}

#[derive(Debug, Clone, Deserialize)]
struct ResponsesOutputItem {
    #[serde(rename = "type")]
    kind: String,
    id: Option<String>,
    call_id: Option<String>,
    name: Option<String>,
    arguments: Option<String>,
    input: Option<String>,
    content: Option<Vec<ResponsesOutputContent>>,
}

#[derive(Debug, Clone, Deserialize)]
struct ResponsesOutputContent {
    text: Option<String>,
}

pub(crate) fn responses_parse_response(body: serde_json::Value) -> Result<CompletionResponse> {
    let body: ResponsesResponseBody = serde_json::from_value(body)?;
    let output = body.output.unwrap_or_default();
    let content = output.iter().find_map(|item| {
        (item.kind == "message").then(|| {
            item.content
                .as_ref()?
                .first()?
                .text
                .as_ref()
                .map(String::from)
        })?
    });
    let mut tool_calls = Vec::new();
    for item in &output {
        if let Some(tool_call) = item.to_tool_call()? {
            tool_calls.push(tool_call);
        }
    }

    let finish_reason = if tool_calls.is_empty() {
        FinishReason::Stop
    } else {
        FinishReason::ToolCalls
    };

    Ok(CompletionResponse {
        response_id: body.id,
        raw_content: content.clone(),
        content,
        reasoning_content: None,
        tool_calls,
        trace_events: Vec::new(),
        next_sequence: 0,
        usage: body
            .usage
            .as_ref()
            .and_then(ProviderTokenUsage::to_responses_usage)
            .unwrap_or_default(),
        finish_reason,
        model: body.model.unwrap_or_default(),
    })
}

impl ResponsesOutputItem {
    fn to_tool_call(&self) -> Result<Option<ToolCall>> {
        match self.kind.as_str() {
            "function_call" => {
                let id = self
                    .id
                    .clone()
                    .ok_or_else(|| response_protocol_error("function_call missing id"))?;
                let name = self
                    .name
                    .clone()
                    .ok_or_else(|| response_protocol_error("function_call missing name"))?;
                let arguments = self
                    .arguments
                    .as_deref()
                    .ok_or_else(|| response_protocol_error("function_call missing arguments"))?;
                Ok(Some(function_tool_call_from_raw(
                    id,
                    name,
                    arguments.to_string(),
                    self.call_id.clone(),
                )))
            }
            "custom_tool_call" => {
                let id = self
                    .id
                    .clone()
                    .or_else(|| self.call_id.clone())
                    .ok_or_else(|| response_protocol_error("custom_tool_call missing id"))?;
                let name = self
                    .name
                    .clone()
                    .ok_or_else(|| response_protocol_error("custom_tool_call missing name"))?;
                let input = self
                    .input
                    .clone()
                    .ok_or_else(|| response_protocol_error("custom_tool_call missing input"))?;
                Ok(Some(ToolCall::custom(
                    id,
                    name,
                    input,
                    self.call_id.clone(),
                )))
            }
            "message"
            | "function_call_output"
            | "custom_tool_call_output"
            | "reasoning"
            | "web_search_call"
            | "file_search_call"
            | "computer_call"
            | "computer_call_output"
            | "mcp_call"
            | "code_interpreter_call" => Ok(None),
            _ => Ok(None),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct ChatResponseBody {
    model: Option<String>,
    choices: Option<Vec<ChatChoice>>,
    usage: Option<ProviderTokenUsage>,
}

#[derive(Debug, Clone, Deserialize)]
struct ChatChoice {
    message: Option<ChatResponseMessage>,
    finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ChatResponseMessage {
    content: Option<String>,
    reasoning_content: Option<String>,
    tool_calls: Option<Vec<ChatResponseToolCall>>,
}

#[derive(Debug, Clone, Deserialize)]
struct ChatResponseToolCall {
    id: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    function: Option<ChatResponseFunctionCall>,
    custom: Option<ChatResponseCustomToolCall>,
}

#[derive(Debug, Clone, Deserialize)]
struct ChatResponseFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ChatResponseCustomToolCall {
    name: String,
    input: String,
}

pub(crate) fn chat_parse_response(body: serde_json::Value) -> Result<CompletionResponse> {
    let body: ChatResponseBody = serde_json::from_value(body)?;
    let choice = body.choices.as_ref().and_then(|choices| choices.first());
    let message = choice.and_then(|choice| choice.message.as_ref());
    let content = message.and_then(|message| message.content.clone());
    let reasoning_content = message.and_then(|message| message.reasoning_content.clone());
    let mut tool_calls = Vec::new();
    if let Some(message_tool_calls) = message.and_then(|message| message.tool_calls.as_ref()) {
        for tool_call in message_tool_calls {
            if let Some(tool_call) = tool_call.to_tool_call()? {
                tool_calls.push(tool_call);
            }
        }
    }
    let finish_reason = choice
        .and_then(|choice| choice.finish_reason.as_deref())
        .map(finish_reason_from_chat)
        .unwrap_or(FinishReason::Stop);

    Ok(CompletionResponse {
        response_id: None,
        raw_content: content.clone(),
        content,
        reasoning_content,
        tool_calls,
        trace_events: Vec::new(),
        next_sequence: 0,
        usage: body
            .usage
            .as_ref()
            .and_then(ProviderTokenUsage::to_chat_usage)
            .unwrap_or_default(),
        finish_reason,
        model: body.model.unwrap_or_default(),
    })
}

impl ChatResponseToolCall {
    fn to_tool_call(&self) -> Result<Option<ToolCall>> {
        match self.kind.as_deref() {
            Some("custom") => {
                let id = self
                    .id
                    .clone()
                    .ok_or_else(|| response_protocol_error("custom tool call missing id"))?;
                let custom = self.custom.as_ref().ok_or_else(|| {
                    response_protocol_error("custom tool call missing custom payload")
                })?;
                Ok(Some(ToolCall::custom(
                    id,
                    custom.name.clone(),
                    custom.input.clone(),
                    None,
                )))
            }
            Some("function") | None => {
                let id = self
                    .id
                    .clone()
                    .ok_or_else(|| response_protocol_error("function tool call missing id"))?;
                let function = self.function.as_ref().ok_or_else(|| {
                    response_protocol_error("function tool call missing function payload")
                })?;
                Ok(Some(function_tool_call_from_raw(
                    id,
                    function.name.clone(),
                    function.arguments.clone(),
                    None,
                )))
            }
            Some(_) => Ok(None),
        }
    }
}

fn response_protocol_error(message: &str) -> PureError {
    PureError::LlmError(format!("provider response protocol error: {message}"))
}

fn finish_reason_from_chat(reason: &str) -> FinishReason {
    match reason {
        "tool_calls" => FinishReason::ToolCalls,
        "length" => FinishReason::MaxTokens,
        "content_filter" => FinishReason::ContentFilter,
        "stop" | "function_call" => FinishReason::Stop,
        _ => FinishReason::Stop,
    }
}
