use pl_protocol::Result;
use serde::Deserialize;

use crate::request::{CompletionResponse, FinishReason, TokenUsage, ToolCall};

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
    let tool_calls = output
        .iter()
        .filter_map(ResponsesOutputItem::to_tool_call)
        .collect::<Vec<_>>();

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
    fn to_tool_call(&self) -> Option<ToolCall> {
        match self.kind.as_str() {
            "function_call" => Some(ToolCall::function(
                self.id.clone()?,
                self.name.clone()?,
                serde_json::from_str(self.arguments.as_deref()?).ok()?,
                self.call_id.clone(),
            )),
            "custom_tool_call" => Some(ToolCall::custom(
                self.id.clone().or_else(|| self.call_id.clone())?,
                self.name.clone()?,
                self.input.clone()?,
                self.call_id.clone(),
            )),
            "message"
            | "function_call_output"
            | "custom_tool_call_output"
            | "reasoning"
            | "web_search_call"
            | "file_search_call"
            | "computer_call"
            | "computer_call_output"
            | "mcp_call"
            | "code_interpreter_call" => None,
            _ => None,
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
    let tool_calls = message
        .and_then(|message| message.tool_calls.as_ref())
        .map(|tool_calls| {
            tool_calls
                .iter()
                .filter_map(ChatResponseToolCall::to_tool_call)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
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
    fn to_tool_call(&self) -> Option<ToolCall> {
        match self.kind.as_deref() {
            Some("custom") => {
                let custom = self.custom.as_ref()?;
                Some(ToolCall::custom(
                    self.id.clone()?,
                    custom.name.clone(),
                    custom.input.clone(),
                    None,
                ))
            }
            Some("function") | None => {
                let function = self.function.as_ref()?;
                Some(ToolCall::function(
                    self.id.clone()?,
                    function.name.clone(),
                    serde_json::from_str(&function.arguments).ok()?,
                    None,
                ))
            }
            Some(_) => None,
        }
    }
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
