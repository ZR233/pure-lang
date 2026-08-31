use pl_protocol::{
    InferenceOrchestrationMetrics, PureError, ResponsesContextItem, Result, ToolCallCaller,
};
use serde::Deserialize;

use crate::completion::tool_arguments::function_tool_call_from_raw;
use crate::completion::{CompletionResponse, ToolCall};

use super::identity::responses_tool_identity;
use super::usage::ProviderTokenUsage;

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
        timing: None,
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
        let identity = responses_tool_identity(self.id.as_deref(), self.call_id.as_deref(), kind);
        if identity.0.is_empty() {
            return Err(response_protocol_error(&format!(
                "{kind} missing id and call_id"
            )));
        }
        Ok(identity)
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
        timing: None,
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
