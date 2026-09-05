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
        accounting: pl_protocol::InferenceAccounting {
            usage: body
                .usage
                .as_ref()
                .and_then(ProviderTokenUsage::to_responses_usage)
                .unwrap_or_default(),
            ..Default::default()
        },
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
        accounting: pl_protocol::InferenceAccounting {
            usage: body
                .usage
                .as_ref()
                .and_then(ProviderTokenUsage::to_chat_usage)
                .unwrap_or_default(),
            ..Default::default()
        },
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

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use crate::completion::ToolCallPayload;
    use crate::runtime::openai::OpenAiProtocol;
    use crate::runtime::openai::test_support::request_with_effort;
    use pl_protocol::{ModelContextItem, PureError, ResponsesContextItemKind, ToolCallCaller};

    #[test]
    fn chat_parse_response_reads_reasoning_content() {
        let response = OpenAiProtocol::chat()
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
    fn responses_parse_response_preserves_orchestration_items_and_caller() {
        let response = OpenAiProtocol::responses()
            .parse_response(serde_json::json!({
                "id": "resp-1",
                "model": "gpt-5.6-sol",
                "output": [
                    {"type": "program", "id": "program-1"},
                    {
                        "type": "function_call",
                        "id": "fc-1",
                        "call_id": "call-1",
                        "name": "git_status",
                        "arguments": "{}",
                        "caller": {"type": "program", "caller_id": "program-1"}
                    }
                ],
                "usage": {"input_tokens": 10, "output_tokens": 2, "total_tokens": 12}
            }))
            .unwrap();

        assert_eq!(response.responses_context_items.len(), 1);
        assert_eq!(response.orchestration.program_count, 1);
        assert_eq!(response.orchestration.program_tool_calls, 1);
        assert_eq!(
            response.tool_calls[0].caller,
            Some(ToolCallCaller::Program {
                caller_id: "program-1".to_string()
            })
        );
    }

    #[test]
    fn responses_parse_response_preserves_unknown_native_items_for_stateless_replay() {
        let response = OpenAiProtocol::responses()
        .parse_response(serde_json::json!({
            "id": "resp-unknown",
            "model": "gpt-5.6-sol",
            "output": [
                {"type": "future_hosted_result", "id": "future-1", "opaque": {"value": 1}},
                {"type": "message", "id": "message-1", "content": [{"type": "output_text", "text": "done"}]}
            ],
            "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2}
        }))
        .unwrap();

        assert_eq!(response.responses_context_items.len(), 1);
        assert_eq!(
            response.responses_context_items[0].kind,
            ResponsesContextItemKind::Unknown
        );
        assert_eq!(
            response.responses_context_items[0].value["type"],
            "future_hosted_result"
        );
    }

    #[test]
    fn responses_parse_response_reads_custom_tool_call() {
        let response = OpenAiProtocol::responses()
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
    fn responses_parse_response_canonicalizes_id_only_tool_identity() {
        let response = OpenAiProtocol::responses()
            .parse_response(serde_json::json!({
                "model": "gpt-5.5",
                "output": [
                    {
                        "type": "function_call",
                        "id": "fc_1",
                        "name": "read_file",
                        "arguments": "{}"
                    },
                    {
                        "type": "custom_tool_call",
                        "id": "ctc_1",
                        "name": "apply_patch",
                        "input": "*** Begin Patch\n*** End Patch"
                    }
                ]
            }))
            .unwrap();

        assert_eq!(response.tool_calls[0].id, "fc_1");
        assert_eq!(response.tool_calls[0].call_id, "fc_1");
        assert_eq!(response.tool_calls[1].id, "ctc_1");
        assert_eq!(response.tool_calls[1].call_id, "ctc_1");
    }

    #[test]
    fn responses_parse_response_uses_call_id_as_missing_item_id() {
        let response = OpenAiProtocol::responses()
            .parse_response(serde_json::json!({
                "model": "gpt-5.5",
                "output": [
                    {
                        "type": "function_call",
                        "call_id": "call_1",
                        "name": "read_file",
                        "arguments": "{}"
                    },
                    {
                        "type": "custom_tool_call",
                        "call_id": "call_2",
                        "name": "apply_patch",
                        "input": "*** Begin Patch\n*** End Patch"
                    }
                ]
            }))
            .unwrap();

        assert_eq!(response.tool_calls[0].id, "call_1");
        assert_eq!(response.tool_calls[0].call_id, "call_1");
        assert_eq!(response.tool_calls[1].id, "call_2");
        assert_eq!(response.tool_calls[1].call_id, "call_2");
    }

    #[test]
    fn responses_parse_response_rejects_empty_tool_identity() {
        let error = OpenAiProtocol::responses()
            .parse_response(serde_json::json!({
                "model": "gpt-5.5",
                "output": [{
                    "type": "function_call",
                    "id": "",
                    "call_id": "",
                    "name": "read_file",
                    "arguments": "{}"
                }]
            }))
            .unwrap_err();

        assert!(matches!(
            error,
            PureError::LlmError(message) if message.contains("missing id and call_id")
        ));
    }

    #[test]
    fn responses_parse_response_preserves_hosted_web_search_context_items() {
        let response = OpenAiProtocol::responses()
            .parse_response(serde_json::json!({
                "id": "resp_1",
                "model": "gpt-5.5",
                "output": [
                    {
                        "type": "web_search_call",
                        "id": "search_1",
                        "action": {"type": "search", "queries": ["alpha", "beta"]},
                        "results": [{"url": "https://example.com/search", "future": 1}]
                    },
                    {
                        "type": "web_search_call",
                        "id": "open_1",
                        "action": {"type": "open_page", "url": "https://example.com/page"}
                    },
                    {
                        "type": "web_search_call",
                        "id": "find_1",
                        "action": {
                            "type": "find_in_page",
                            "url": "https://example.com/page",
                            "pattern": "needle"
                        }
                    },
                    {
                        "type": "web_search_call",
                        "id": "future_1",
                        "action": {"type": "future_action", "opaque": true}
                    }
                ]
            }))
            .unwrap();

        assert_eq!(response.responses_context_items.len(), 4);
        assert_eq!(
            response.responses_context_items[0].value["action"]["queries"],
            serde_json::json!(["alpha", "beta"])
        );
        assert_eq!(
            response.responses_context_items[0].value["results"][0]["future"],
            1
        );
        assert_eq!(
            response.responses_context_items[1].value["action"]["url"],
            "https://example.com/page"
        );
        assert_eq!(
            response.responses_context_items[2].value["action"]["pattern"],
            "needle"
        );
        assert_eq!(
            response.responses_context_items[3].value["action"]["type"],
            "future_action"
        );

        let expected = response
            .responses_context_items
            .iter()
            .map(|item| item.value.clone())
            .collect::<Vec<_>>();
        let mut request = request_with_effort("high");
        request.input = response
            .responses_context_items
            .into_iter()
            .map(|item| ModelContextItem::Responses { item })
            .collect();
        let body = OpenAiProtocol::responses().build_request_body(&request);

        assert_eq!(body["input"], serde_json::json!(expected));
    }

    #[test]
    fn responses_parse_response_preserves_invalid_function_arguments() {
        let response = OpenAiProtocol::responses()
            .parse_response(serde_json::json!({
                "model": "gpt-5.5",
                "output": [{
                    "type": "function_call",
                    "id": "fc_1",
                    "call_id": "call_1",
                    "name": "read_file",
                    "arguments": "{bad"
                }]
            }))
            .unwrap();

        let call = &response.tool_calls[0];
        assert_eq!(call.payload_text(), "{bad");
        assert_eq!(call.invalid_arguments.as_ref().unwrap().raw, "{bad");
        assert!(
            call.invalid_arguments_message()
                .unwrap()
                .contains("read_file")
        );
    }

    #[test]
    fn chat_parse_response_reads_custom_tool_call() {
        let response = OpenAiProtocol::chat()
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
    fn chat_parse_response_preserves_invalid_function_arguments() {
        let response = OpenAiProtocol::chat()
            .parse_response(serde_json::json!({
                "model": "gpt-5.5",
                "choices": [{
                    "message": {
                        "role": "assistant",
                        "tool_calls": [{
                            "id": "call_1",
                            "type": "function",
                            "function": {
                                "name": "read_file",
                                "arguments": "{bad"
                            }
                        }]
                    },
                    "finish_reason": "tool_calls"
                }]
            }))
            .unwrap();

        let call = &response.tool_calls[0];
        assert_eq!(call.payload_text(), "{bad");
        assert_eq!(call.invalid_arguments.as_ref().unwrap().raw, "{bad");
        assert!(
            call.invalid_arguments_message()
                .unwrap()
                .contains("read_file")
        );
    }
}
