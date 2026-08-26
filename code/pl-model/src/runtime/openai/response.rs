use pl_protocol::{
    InferenceOrchestrationMetrics, PureError, ResponsesContextItem, Result, ToolCallCaller,
};
use serde::Deserialize;

use crate::completion::tool_arguments::function_tool_call_from_raw;
use crate::completion::{CompletionResponse, TokenUsage, ToolCall};

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

    fn cache_write_tokens(&self) -> u64 {
        self.input_tokens_details
            .as_ref()
            .and_then(TokenUsageDetails::cache_write)
            .or_else(|| {
                self.prompt_tokens_details
                    .as_ref()
                    .and_then(TokenUsageDetails::cache_write)
            })
            .unwrap_or(0)
    }

    fn to_responses_usage(&self) -> Option<TokenUsage> {
        Some(TokenUsage {
            prompt_tokens: self.input_tokens?,
            completion_tokens: self.output_tokens?,
            total_tokens: self.total_tokens.unwrap_or(0),
            cached_prompt_tokens: self.cached_prompt_tokens(),
            cache_write_tokens: self.cache_write_tokens(),
            reasoning_tokens: self.reasoning_tokens(),
        })
    }

    fn to_chat_usage(&self) -> Option<TokenUsage> {
        Some(TokenUsage {
            prompt_tokens: self.prompt_tokens?,
            completion_tokens: self.completion_tokens?,
            total_tokens: self.total_tokens.unwrap_or(0),
            cached_prompt_tokens: self.cached_prompt_tokens(),
            cache_write_tokens: self.cache_write_tokens(),
            reasoning_tokens: self.reasoning_tokens(),
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
struct TokenUsageDetails {
    cached_tokens: Option<u64>,
    cache_read_tokens: Option<u64>,
    cached_input_tokens: Option<u64>,
    cache_write_tokens: Option<u64>,
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

    fn cache_write(&self) -> Option<u64> {
        self.cache_write_tokens
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
    caller: Option<ToolCallCaller>,
}

#[derive(Debug, Clone, Deserialize)]
struct ResponsesOutputContent {
    text: Option<String>,
}

pub(crate) fn responses_parse_response(body: serde_json::Value) -> Result<CompletionResponse> {
    let raw_output = body
        .get("output")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
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
    let responses_context_items = raw_output
        .iter()
        .cloned()
        .filter_map(ResponsesContextItem::from_wire)
        .collect::<Vec<_>>();
    let orchestration = responses_orchestration_metrics(&raw_output, &tool_calls);

    Ok(CompletionResponse {
        response_id: body.id,
        content,
        reasoning_content: None,
        tool_calls,
        responses_context_items,
        orchestration,
        usage: body
            .usage
            .as_ref()
            .and_then(ProviderTokenUsage::to_responses_usage)
            .unwrap_or_default(),
        model: body.model.unwrap_or_default(),
    })
}

impl ResponsesOutputItem {
    fn to_tool_call(&self) -> Result<Option<ToolCall>> {
        match self.kind.as_str() {
            "function_call" => {
                let (id, call_id) = self.tool_identity("function_call")?;
                let name = self
                    .name
                    .clone()
                    .ok_or_else(|| response_protocol_error("function_call missing name"))?;
                let arguments = self
                    .arguments
                    .as_deref()
                    .ok_or_else(|| response_protocol_error("function_call missing arguments"))?;
                Ok(Some(
                    function_tool_call_from_raw(id, name, arguments.to_string(), call_id)
                        .with_caller(self.caller.clone()),
                ))
            }
            "custom_tool_call" => {
                let (id, call_id) = self.tool_identity("custom_tool_call")?;
                let name = self
                    .name
                    .clone()
                    .ok_or_else(|| response_protocol_error("custom_tool_call missing name"))?;
                let input = self
                    .input
                    .clone()
                    .ok_or_else(|| response_protocol_error("custom_tool_call missing input"))?;
                Ok(Some(
                    ToolCall::custom(id, name, input, call_id).with_caller(self.caller.clone()),
                ))
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

    fn tool_identity(&self, kind: &str) -> Result<(String, String)> {
        let item_id = self
            .id
            .as_deref()
            .filter(|id| !id.is_empty())
            .or_else(|| {
                self.call_id
                    .as_deref()
                    .filter(|call_id| !call_id.is_empty())
            })
            .ok_or_else(|| response_protocol_error(&format!("{kind} missing id and call_id")))?;
        let call_id = self
            .call_id
            .as_deref()
            .filter(|call_id| !call_id.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| {
                tracing::trace!(
                    item_id,
                    kind,
                    "responses tool item missing call_id; assigning item id"
                );
                item_id.to_string()
            });
        Ok((item_id.to_string(), call_id))
    }
}

fn responses_orchestration_metrics(
    output: &[serde_json::Value],
    tool_calls: &[ToolCall],
) -> InferenceOrchestrationMetrics {
    let program_count = output
        .iter()
        .filter(|item| item.get("type").and_then(serde_json::Value::as_str) == Some("program"))
        .count() as u64;
    let program_tool_calls = tool_calls
        .iter()
        .filter(|call| call.caller.is_some())
        .count() as u64;
    InferenceOrchestrationMetrics {
        tool_calls: tool_calls.len() as u64,
        program_count,
        program_tool_calls,
        transport_attempts: 1,
        ..InferenceOrchestrationMetrics::default()
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
    _finish_reason: Option<String>,
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
    let orchestration = InferenceOrchestrationMetrics {
        tool_calls: tool_calls.len() as u64,
        transport_attempts: 1,
        ..InferenceOrchestrationMetrics::default()
    };

    Ok(CompletionResponse {
        response_id: None,
        content,
        reasoning_content,
        tool_calls,
        responses_context_items: Vec::new(),
        orchestration,
        usage: body
            .usage
            .as_ref()
            .and_then(ProviderTokenUsage::to_chat_usage)
            .unwrap_or_default(),
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
                    .filter(|id| !id.is_empty())
                    .ok_or_else(|| response_protocol_error("custom tool call missing id"))?;
                let custom = self.custom.as_ref().ok_or_else(|| {
                    response_protocol_error("custom tool call missing custom payload")
                })?;
                Ok(Some(ToolCall::custom(
                    id.clone(),
                    custom.name.clone(),
                    custom.input.clone(),
                    // Chat Completions 只暴露 item id；确定性赋 call_id = item_id。
                    id,
                )))
            }
            Some("function") | None => {
                let id = self
                    .id
                    .clone()
                    .filter(|id| !id.is_empty())
                    .ok_or_else(|| response_protocol_error("function tool call missing id"))?;
                let function = self.function.as_ref().ok_or_else(|| {
                    response_protocol_error("function tool call missing function payload")
                })?;
                Ok(Some(function_tool_call_from_raw(
                    id.clone(),
                    function.name.clone(),
                    function.arguments.clone(),
                    // Chat Completions 只暴露 item id；确定性赋 call_id = item_id。
                    id,
                )))
            }
            Some(_) => Ok(None),
        }
    }
}

fn response_protocol_error(message: &str) -> PureError {
    PureError::LlmError(format!("provider response protocol error: {message}"))
}
