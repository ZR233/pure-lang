use std::collections::HashMap;

use pl_protocol::{
    ContentPart, ImageSource, Message, MessageContent, MessageRole, ModelContextItem, PureError,
};
use pretty_assertions::assert_eq;

use super::*;
use crate::model_info::{MaxTokensField, ResponsesMaxTokensField};
use crate::request::{ReasoningConfig, ReasoningSummary, ToolCall, ToolCallPayload, ToolSchema};

fn text_message(role: MessageRole, content: &str) -> Message {
    Message {
        role,
        content: MessageContent::Text(content.to_string()),
        reasoning_content: None,
        metadata: HashMap::new(),
    }
}

fn image_message() -> Message {
    Message {
        role: MessageRole::User,
        content: MessageContent::MultiPart(vec![
            ContentPart::Text {
                text: "describe".to_string(),
            },
            ContentPart::Image {
                source: ImageSource::InlineBase64 {
                    data: "aGVsbG8=".to_string(),
                },
                media_type: "image/png".to_string(),
                filename: Some("sample.png".to_string()),
            },
        ]),
        reasoning_content: None,
        metadata: HashMap::new(),
    }
}

fn context_items(messages: Vec<Message>) -> Vec<ModelContextItem> {
    messages.into_iter().map(ModelContextItem::from).collect()
}

fn request_with_effort(effort: &str) -> CompletionRequest {
    CompletionRequest {
        model: "gpt-5.5".to_string(),
        instructions: None,
        input: context_items(vec![text_message(MessageRole::User, "hello")]),
        tools: Vec::new(),
        tool_choice: "auto".to_string(),
        parallel_tool_calls: true,
        temperature: None,
        max_tokens: None,
        store: None,
        previous_response_id: None,
        prompt_cache_key: None,
        reasoning: Some(ReasoningConfig {
            effort: Some(effort.to_string()),
            summary: None,
        }),
        stream: true,
        trace: None,
    }
}

#[test]
fn responses_use_top_level_instructions_and_developer_messages() {
    let request = CompletionRequest {
        model: "gpt-5.5".to_string(),
        instructions: Some("base".to_string()),
        input: context_items(vec![
            text_message(MessageRole::System, "developer"),
            text_message(MessageRole::User, "user context"),
            text_message(MessageRole::User, "real prompt"),
        ]),
        tools: Vec::new(),
        tool_choice: "auto".to_string(),
        parallel_tool_calls: false,
        temperature: None,
        max_tokens: None,
        store: None,
        previous_response_id: None,
        prompt_cache_key: None,
        reasoning: None,
        stream: true,
        trace: None,
    };

    let responses_body = OpenAiProtocol::responses().build_request_body(&request);
    let chat_body = OpenAiProtocol::chat().build_request_body(&request);

    assert_eq!(responses_body["instructions"], serde_json::json!("base"),);
    assert_eq!(
        responses_body["input"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["role"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["developer", "user", "user"],
    );
    assert_eq!(
        chat_body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["role"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["system", "system", "user", "user"],
    );
    assert_eq!(
        chat_body["messages"][0]["content"],
        serde_json::json!("base"),
    );
}

#[test]
fn responses_maps_image_parts_to_input_image() {
    let request = CompletionRequest {
        model: "gpt-5.5".to_string(),
        instructions: None,
        input: context_items(vec![image_message()]),
        tools: Vec::new(),
        tool_choice: "auto".to_string(),
        parallel_tool_calls: false,
        temperature: None,
        max_tokens: None,
        store: None,
        previous_response_id: None,
        prompt_cache_key: None,
        reasoning: None,
        stream: true,
        trace: None,
    };

    let body = OpenAiProtocol::responses().build_request_body(&request);

    assert_eq!(body["input"][0]["content"][0]["type"], "input_text");
    assert_eq!(body["input"][0]["content"][0]["text"], "describe");
    assert_eq!(body["input"][0]["content"][1]["type"], "input_image");
    assert_eq!(
        body["input"][0]["content"][1]["image_url"],
        "data:image/png;base64,aGVsbG8="
    );
}

#[test]
fn chat_maps_image_parts_to_content_array() {
    let request = CompletionRequest {
        model: "glm-5v".to_string(),
        instructions: None,
        input: context_items(vec![image_message()]),
        tools: Vec::new(),
        tool_choice: "auto".to_string(),
        parallel_tool_calls: false,
        temperature: None,
        max_tokens: None,
        store: None,
        previous_response_id: None,
        prompt_cache_key: None,
        reasoning: None,
        stream: true,
        trace: None,
    };

    let body = OpenAiProtocol::chat().build_request_body(&request);

    assert_eq!(body["messages"][0]["content"][0]["type"], "text");
    assert_eq!(body["messages"][0]["content"][1]["type"], "image_url");
    assert_eq!(
        body["messages"][0]["content"][1]["image_url"]["url"],
        "data:image/png;base64,aGVsbG8="
    );
}

fn request_with_tool_history(tool_metadata: HashMap<String, String>) -> CompletionRequest {
    let calls = vec![ToolCall::custom(
        "ctc_1",
        "apply_patch",
        "*** Begin Patch\n*** End Patch",
        Some("call_1".to_string()),
    )];
    let mut assistant_metadata = HashMap::new();
    assistant_metadata.insert(
        "tool_calls".to_string(),
        serde_json::to_string(&calls).unwrap(),
    );
    CompletionRequest {
        model: "gpt-5.5".to_string(),
        instructions: None,
        input: context_items(vec![
            Message {
                role: MessageRole::Assistant,
                content: MessageContent::Text(String::new()),
                reasoning_content: None,
                metadata: assistant_metadata,
            },
            Message {
                role: MessageRole::Tool,
                content: MessageContent::Text("ok".to_string()),
                reasoning_content: None,
                metadata: tool_metadata,
            },
        ]),
        tools: Vec::new(),
        tool_choice: "auto".to_string(),
        parallel_tool_calls: false,
        temperature: None,
        max_tokens: None,
        store: None,
        previous_response_id: None,
        prompt_cache_key: None,
        reasoning: None,
        stream: true,
        trace: None,
    }
}

fn request_with_function_tool_history(tool_metadata: HashMap<String, String>) -> CompletionRequest {
    let calls = vec![ToolCall::function(
        "fc_1",
        "read_file",
        serde_json::json!({ "path": "Cargo.toml" }),
        Some("call_1".to_string()),
    )];
    let mut assistant_metadata = HashMap::new();
    assistant_metadata.insert(
        "tool_calls".to_string(),
        serde_json::to_string(&calls).unwrap(),
    );
    CompletionRequest {
        model: "gpt-5.5".to_string(),
        instructions: None,
        input: context_items(vec![
            Message {
                role: MessageRole::Assistant,
                content: MessageContent::Text(String::new()),
                reasoning_content: None,
                metadata: assistant_metadata,
            },
            Message {
                role: MessageRole::Tool,
                content: MessageContent::Text("ok".to_string()),
                reasoning_content: None,
                metadata: tool_metadata,
            },
        ]),
        tools: Vec::new(),
        tool_choice: "auto".to_string(),
        parallel_tool_calls: false,
        temperature: None,
        max_tokens: None,
        store: None,
        previous_response_id: None,
        prompt_cache_key: None,
        reasoning: None,
        stream: true,
        trace: None,
    }
}

fn bundled_model(slug: &str) -> ModelInfo {
    crate::default_models::default_models()
        .into_iter()
        .find(|model| model.slug == slug)
        .unwrap_or_else(|| panic!("test bundled model not found: {slug}"))
}

#[test]
fn responses_body_writes_effort_via_parameter_wire() {
    let model = bundled_model("gpt-5.5");
    let body = OpenAiProtocol::responses()
        .build_request_body_with_model(&request_with_effort("high"), &model);

    assert_eq!(body["reasoning"]["effort"], serde_json::json!("high"));
}

#[test]
fn responses_body_maps_enabled_reasoning_summary_to_auto() {
    let model = bundled_model("gpt-5.5");
    let mut request = request_with_effort("medium");
    request.reasoning.as_mut().unwrap().summary = Some(ReasoningSummary::Enabled);

    let body = OpenAiProtocol::responses().build_request_body_with_model(&request, &model);

    assert_eq!(body["reasoning"]["summary"], serde_json::json!("auto"));
}

#[test]
fn responses_body_omits_disabled_reasoning_summary() {
    let model = bundled_model("gpt-5.5");
    let mut request = request_with_effort("medium");
    request.reasoning.as_mut().unwrap().summary = Some(ReasoningSummary::Disabled);

    let body = OpenAiProtocol::responses().build_request_body_with_model(&request, &model);

    assert!(body["reasoning"].get("summary").is_none());
}

#[test]
fn responses_body_writes_continuation_fields() {
    let mut request = request_with_effort("medium");
    request.store = Some(true);
    request.previous_response_id = Some("resp_previous".to_string());
    request.prompt_cache_key = Some("project-cache-key".to_string());

    let responses_body = OpenAiProtocol::responses().build_request_body(&request);
    let chat_body = OpenAiProtocol::chat().build_request_body(&request);

    assert_eq!(responses_body["store"], serde_json::json!(true));
    assert_eq!(
        responses_body["previous_response_id"],
        serde_json::json!("resp_previous")
    );
    assert_eq!(
        responses_body["prompt_cache_key"],
        serde_json::json!("project-cache-key")
    );
    assert!(chat_body.get("store").is_none());
    assert!(chat_body.get("previous_response_id").is_none());
    assert!(chat_body.get("prompt_cache_key").is_none());
}

#[test]
fn responses_body_omits_profiled_max_tokens_by_default() {
    let mut request = request_with_effort("medium");
    request.max_tokens = Some(8192);

    let body = OpenAiProtocol::responses().build_request_body(&request);

    assert!(body.get("max_output_tokens").is_none());
    assert!(body.get("max_tokens").is_none());
    assert!(body.get("max_completion_tokens").is_none());
}

#[test]
fn responses_body_can_use_profiled_max_output_tokens_field() {
    let mut model = ModelInfo::fallback("responses-like");
    model.request_profile.responses_max_tokens_field = ResponsesMaxTokensField::MaxOutputTokens;
    let mut request = request_with_effort("medium");
    request.model = "responses-like".to_string();
    request.max_tokens = Some(8192);

    let body = OpenAiProtocol::responses().build_request_body_with_model(&request, &model);

    assert_eq!(body["max_output_tokens"], serde_json::json!(8192));
    assert!(body.get("max_tokens").is_none());
    assert!(body.get("max_completion_tokens").is_none());
}

#[test]
fn responses_body_can_use_profiled_compatible_max_tokens_fields() {
    let mut request = request_with_effort("medium");
    request.model = "responses-like".to_string();
    request.max_tokens = Some(8192);

    let mut max_tokens_model = ModelInfo::fallback("responses-like");
    max_tokens_model.request_profile.responses_max_tokens_field =
        ResponsesMaxTokensField::MaxTokens;
    let max_tokens_body =
        OpenAiProtocol::responses().build_request_body_with_model(&request, &max_tokens_model);

    let mut max_completion_model = ModelInfo::fallback("responses-like");
    max_completion_model
        .request_profile
        .responses_max_tokens_field = ResponsesMaxTokensField::MaxCompletionTokens;
    let max_completion_body =
        OpenAiProtocol::responses().build_request_body_with_model(&request, &max_completion_model);

    assert_eq!(max_tokens_body["max_tokens"], serde_json::json!(8192));
    assert!(max_tokens_body.get("max_output_tokens").is_none());
    assert_eq!(
        max_completion_body["max_completion_tokens"],
        serde_json::json!(8192)
    );
    assert!(max_completion_body.get("max_output_tokens").is_none());
}

#[test]
fn chat_body_can_use_profiled_max_completion_tokens_field() {
    let mut model = ModelInfo::fallback("mimo-chat");
    model.request_profile.max_tokens_field = MaxTokensField::MaxCompletionTokens;
    let mut request = request_with_effort("high");
    request.model = "mimo-chat".to_string();
    request.max_tokens = Some(8192);

    let body = OpenAiProtocol::chat().build_request_body_with_model(&request, &model);

    assert!(body.get("max_tokens").is_none());
    assert_eq!(body["max_completion_tokens"], serde_json::json!(8192));
}

#[test]
fn chat_body_without_effort_parameter_omits_reasoning_fields() {
    let body = OpenAiProtocol::chat().build_request_body(&request_with_effort("max"));

    assert!(body.get("reasoning_effort").is_none());
    assert!(body.get("thinking").is_none());
}

#[test]
fn deepseek_chat_body_writes_effort_and_base_body_thinking() {
    let model = bundled_model("deepseek-v4-flash");
    let body =
        OpenAiProtocol::chat().build_request_body_with_model(&request_with_effort("max"), &model);

    assert_eq!(body["reasoning_effort"], serde_json::json!("max"));
    assert_eq!(body["thinking"]["type"], serde_json::json!("enabled"));
}

#[test]
fn zhipu_plain_chat_body_maps_effort_to_thinking_type() {
    let model = bundled_model("glm-5");
    let body = OpenAiProtocol::chat()
        .build_request_body_with_model(&request_with_effort("enabled"), &model);

    assert!(body.get("reasoning_effort").is_none());
    assert_eq!(body["thinking"]["type"], serde_json::json!("enabled"));
    assert_eq!(body["thinking"]["clear_thinking"], serde_json::json!(false));
}

#[test]
fn glm52_chat_body_links_reasoning_effort_and_thinking() {
    let model = bundled_model("glm-5.2");
    for effort in ["high", "max"] {
        let body = OpenAiProtocol::chat()
            .build_request_body_with_model(&request_with_effort(effort), &model);

        assert_eq!(body["reasoning_effort"], serde_json::json!(effort));
        assert_eq!(body["thinking"]["type"], serde_json::json!("enabled"));
        assert_eq!(body["thinking"]["clear_thinking"], serde_json::json!(false));
    }
}

#[test]
fn glm52_chat_body_none_disables_thinking_and_removes_reasoning_effort() {
    let model = bundled_model("glm-5.2");
    let body =
        OpenAiProtocol::chat().build_request_body_with_model(&request_with_effort("none"), &model);

    assert!(body.get("reasoning_effort").is_none());
    assert_eq!(body["thinking"]["type"], serde_json::json!("disabled"));
    assert!(body["thinking"].get("clear_thinking").is_none());
}

#[test]
fn chat_body_writes_assistant_reasoning_content() {
    let mut request = request_with_effort("high");
    request.input = vec![pl_protocol::ModelContextItem::from(Message {
        role: MessageRole::Assistant,
        content: MessageContent::Text("9.11 更大。".to_string()),
        reasoning_content: Some("比较小数位。".to_string()),
        metadata: HashMap::new(),
    })];

    let body = OpenAiProtocol::chat().build_request_body(&request);

    assert_eq!(
        body["messages"][0]["reasoning_content"],
        serde_json::json!("比较小数位。")
    );
}

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
fn chat_parse_response_reads_cached_prompt_tokens() {
    let response = OpenAiProtocol::chat()
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
    let response = OpenAiProtocol::responses()
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

    let body = OpenAiProtocol::responses().build_request_body(&request);

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

    let body = OpenAiProtocol::chat().build_request_body(&request);

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
    let body = OpenAiProtocol::chat().build_request_body(&request);

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
    assert!(description.contains("*** File: metadata"));
    assert!(description.contains("Insert after"));
    assert!(description.contains("previous patch failed"));
    assert!(description.contains("Minimal update example:"));
    assert!(description.contains("*** Update File: notes.txt"));
    assert!(description.contains("-old line"));
    assert!(description.contains("+new line"));
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
fn responses_parse_response_rejects_invalid_function_arguments() {
    let error = OpenAiProtocol::responses()
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
        .unwrap_err();

    match error {
        PureError::LlmError(message) => {
            assert!(message.contains("invalid JSON arguments"));
            assert!(message.contains("read_file"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
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
fn chat_parse_response_rejects_invalid_function_arguments() {
    let error = OpenAiProtocol::chat()
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
        .unwrap_err();

    match error {
        PureError::LlmError(message) => {
            assert!(message.contains("invalid JSON arguments"));
            assert!(message.contains("read_file"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn responses_history_replays_custom_tool_call_and_output() {
    let mut tool_metadata = HashMap::new();
    tool_metadata.insert("tool_call_id".to_string(), "ctc_1".to_string());
    tool_metadata.insert("tool_call_call_id".to_string(), "call_1".to_string());
    tool_metadata.insert("tool_call_kind".to_string(), "custom".to_string());
    tool_metadata.insert("tool_name".to_string(), "apply_patch".to_string());
    let request = request_with_tool_history(tool_metadata);

    let body = OpenAiProtocol::responses().build_request_body(&request);

    assert_eq!(
        body["input"][0]["type"],
        serde_json::json!("custom_tool_call")
    );
    assert!(body["input"][0]["id"].is_null());
    assert_eq!(body["input"][0]["call_id"], serde_json::json!("call_1"));
    assert_eq!(
        body["input"][1]["type"],
        serde_json::json!("custom_tool_call_output")
    );
    assert!(
        !body["input"][1]
            .as_object()
            .expect("custom tool output should serialize as object")
            .contains_key("name")
    );
}

#[test]
fn tool_result_ids_are_protocol_specific() {
    let mut tool_metadata = HashMap::new();
    tool_metadata.insert("tool_call_id".to_string(), "ctc_1".to_string());
    tool_metadata.insert("tool_call_call_id".to_string(), "call_1".to_string());
    tool_metadata.insert("tool_call_kind".to_string(), "custom".to_string());
    tool_metadata.insert("tool_name".to_string(), "apply_patch".to_string());
    let request = request_with_tool_history(tool_metadata);

    let responses_body = OpenAiProtocol::responses().build_request_body(&request);
    let chat_body = OpenAiProtocol::chat().build_request_body(&request);

    assert_eq!(
        responses_body["input"][1]["call_id"],
        serde_json::json!("call_1")
    );
    assert!(responses_body["input"][0]["id"].is_null());
    assert_eq!(
        chat_body["messages"][1]["tool_call_id"],
        serde_json::json!("ctc_1")
    );
}

#[test]
fn function_tool_result_ids_are_protocol_specific() {
    let mut tool_metadata = HashMap::new();
    tool_metadata.insert("tool_call_id".to_string(), "fc_1".to_string());
    tool_metadata.insert("tool_call_call_id".to_string(), "call_1".to_string());
    tool_metadata.insert("tool_call_kind".to_string(), "function".to_string());
    tool_metadata.insert("tool_name".to_string(), "read_file".to_string());
    let request = request_with_function_tool_history(tool_metadata);

    let responses_body = OpenAiProtocol::responses().build_request_body(&request);
    let chat_body = OpenAiProtocol::chat().build_request_body(&request);

    assert_eq!(
        responses_body["input"][1]["call_id"],
        serde_json::json!("call_1")
    );
    assert!(responses_body["input"][0]["id"].is_null());
    assert_eq!(
        chat_body["messages"][1]["tool_call_id"],
        serde_json::json!("fc_1")
    );
}

#[test]
fn unknown_tool_call_kind_fails_request_build() {
    let mut tool_metadata = HashMap::new();
    tool_metadata.insert("tool_call_id".to_string(), "fc_1".to_string());
    tool_metadata.insert("tool_call_kind".to_string(), "mystery".to_string());
    tool_metadata.insert("tool_name".to_string(), "read_file".to_string());
    let request = request_with_function_tool_history(tool_metadata);

    let error = OpenAiProtocol::responses()
        .build_request(&request, &ModelInfo::fallback(&request.model))
        .unwrap_err();

    match error {
        PureError::LlmError(message) => {
            assert!(message.contains("unknown tool_call_kind: mystery"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn missing_tool_output_fails_request_build() {
    let calls = vec![ToolCall::function(
        "fc_1",
        "read_file",
        serde_json::json!({ "path": "Cargo.toml" }),
        Some("call_1".to_string()),
    )];
    let mut assistant_metadata = HashMap::new();
    assistant_metadata.insert(
        "tool_calls".to_string(),
        serde_json::to_string(&calls).unwrap(),
    );
    let request = CompletionRequest {
        model: "gpt-5.5".to_string(),
        instructions: None,
        input: context_items(vec![Message {
            role: MessageRole::Assistant,
            content: MessageContent::Text(String::new()),
            reasoning_content: None,
            metadata: assistant_metadata,
        }]),
        tools: Vec::new(),
        tool_choice: "auto".to_string(),
        parallel_tool_calls: false,
        temperature: None,
        max_tokens: None,
        store: None,
        previous_response_id: None,
        prompt_cache_key: None,
        reasoning: None,
        stream: true,
        trace: None,
    };

    let error = OpenAiProtocol::responses()
        .build_request(&request, &ModelInfo::fallback(&request.model))
        .unwrap_err();

    match error {
        PureError::LlmError(message) => {
            assert!(message.contains("assistant tool call fc_1 is missing tool output"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn responses_history_requires_call_id_but_chat_uses_tool_call_id() {
    let calls = vec![ToolCall::function(
        "fc_1",
        "read_file",
        serde_json::json!({ "path": "Cargo.toml" }),
        None,
    )];
    let mut assistant_metadata = HashMap::new();
    assistant_metadata.insert(
        "tool_calls".to_string(),
        serde_json::to_string(&calls).unwrap(),
    );
    let mut tool_metadata = HashMap::new();
    tool_metadata.insert("tool_call_id".to_string(), "fc_1".to_string());
    tool_metadata.insert("tool_call_kind".to_string(), "function".to_string());
    tool_metadata.insert("tool_name".to_string(), "read_file".to_string());
    let request = CompletionRequest {
        model: "gpt-5.5".to_string(),
        instructions: None,
        input: context_items(vec![
            Message {
                role: MessageRole::Assistant,
                content: MessageContent::Text(String::new()),
                reasoning_content: None,
                metadata: assistant_metadata,
            },
            Message {
                role: MessageRole::Tool,
                content: MessageContent::Text("ok".to_string()),
                reasoning_content: None,
                metadata: tool_metadata,
            },
        ]),
        tools: Vec::new(),
        tool_choice: "auto".to_string(),
        parallel_tool_calls: false,
        temperature: None,
        max_tokens: None,
        store: None,
        previous_response_id: None,
        prompt_cache_key: None,
        reasoning: None,
        stream: true,
        trace: None,
    };

    let responses_error = OpenAiProtocol::responses()
        .build_request(&request, &ModelInfo::fallback(&request.model))
        .unwrap_err();
    let chat_body = OpenAiProtocol::chat().build_request_body(&request);

    match responses_error {
        PureError::LlmError(message) => {
            assert!(message.contains("missing call_id for Responses history replay"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
    assert_eq!(
        chat_body["messages"][1]["tool_call_id"],
        serde_json::json!("fc_1")
    );
}
