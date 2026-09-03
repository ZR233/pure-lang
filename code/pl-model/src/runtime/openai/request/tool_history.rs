use std::collections::{HashMap, VecDeque};

use pl_protocol::{Message, MessageRole, Result, ToolCallCaller, ToolCallKind};

use super::protocol_error;

/// 校验 assistant tool call 与 tool result 的 typed 配对。
///
/// `call_id` 与 `item_id` 必填由解码与写入边界保证，这里不重复检查空值；
/// 校验聚焦 id 配对、kind 配对和缺失 output；开头的 tool result 一律拒绝。
pub(super) fn validate_tool_history(messages: &[Message]) -> Result<()> {
    let mut expected_outputs = VecDeque::new();

    for message in messages {
        match message.role {
            MessageRole::Assistant => {
                for tool_call in message.tool_calls.iter().flatten() {
                    expected_outputs.push_back(ExpectedToolOutput {
                        item_id: tool_call.item_id.clone(),
                        call_id: tool_call.call_id.clone(),
                        kind: tool_call.kind,
                    });
                }
            }
            MessageRole::Tool => {
                let Some(tool_result) = message.tool_result.as_ref() else {
                    return Err(protocol_error(
                        "tool result message missing typed tool_result record",
                    ));
                };
                let Some(expected) = expected_outputs.pop_front() else {
                    return Err(protocol_error(
                        "tool result has no preceding assistant tool call",
                    ));
                };
                if tool_result.item_id != expected.item_id {
                    return Err(protocol_error(format!(
                        "tool result item_id {} does not match assistant tool call item_id {}",
                        tool_result.item_id, expected.item_id
                    )));
                }
                if tool_result.call_id != expected.call_id {
                    return Err(protocol_error(format!(
                        "tool result call_id {} does not match assistant tool call call_id {}",
                        tool_result.call_id, expected.call_id
                    )));
                }
                if tool_result.kind != expected.kind {
                    return Err(protocol_error(format!(
                        "tool result kind {} does not match assistant tool call kind {}",
                        tool_result.kind.as_str(),
                        expected.kind.as_str()
                    )));
                }
            }
            MessageRole::System | MessageRole::User => {}
        }
    }

    if let Some(expected) = expected_outputs.front() {
        return Err(protocol_error(format!(
            "assistant tool call {} is missing tool output",
            expected.item_id
        )));
    }

    Ok(())
}

/// 按 `call_id` 收集 assistant 工具调用声明的 Programmatic caller。
///
/// tool result 侧的 typed 记录不重复保存 caller；回放 `function_call_output` 时
/// 以 call_id 关联 assistant 侧调用。
pub(super) fn tool_callers_by_call_id(messages: &[Message]) -> HashMap<String, ToolCallCaller> {
    messages
        .iter()
        .filter_map(|message| message.tool_calls.as_ref())
        .flatten()
        .filter_map(|tool_call| {
            tool_call
                .caller
                .clone()
                .map(|caller| (tool_call.call_id.clone(), caller))
        })
        .collect()
}

/// 把 typed 记录中的参数投影为 provider wire 文本。
///
/// 字符串字面量表示 custom 输入或未解析的原始 function 参数，按原文发送；
/// 其余 JSON 值重新序列化为紧凑文本。
pub(super) fn record_arguments_text(arguments: &serde_json::Value) -> String {
    match arguments {
        serde_json::Value::String(raw) => raw.clone(),
        value => serde_json::to_string(value).unwrap_or_default(),
    }
}

/// custom 工具调用的输入文本。
pub(super) fn record_custom_input(arguments: &serde_json::Value) -> String {
    arguments.as_str().unwrap_or_default().to_string()
}

struct ExpectedToolOutput {
    item_id: String,
    call_id: String,
    kind: ToolCallKind,
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::completion::CompletionRequest;
    use crate::model::info::ModelInfo;
    use crate::runtime::openai::OpenAiProtocol;
    use crate::runtime::openai::test_support::{context_items, request_with_effort};
    use pl_protocol::{
        Message, MessageContent, MessageRole, ModelContextItem, PureError, ResponsesContextItem,
        ResponsesContextItemKind, ToolCallCaller, ToolCallKind, ToolCallRecord, ToolResultRecord,
    };

    fn custom_tool_call_record() -> ToolCallRecord {
        ToolCallRecord {
            item_id: "ctc_1".to_string(),
            call_id: "call_1".to_string(),
            name: "apply_patch".to_string(),
            kind: ToolCallKind::Custom,
            arguments: serde_json::Value::String("*** Begin Patch\n*** End Patch".to_string()),
            caller: None,
        }
    }

    fn function_tool_call_record() -> ToolCallRecord {
        ToolCallRecord {
            item_id: "fc_1".to_string(),
            call_id: "call_1".to_string(),
            name: "read_file".to_string(),
            kind: ToolCallKind::Function,
            arguments: serde_json::json!({ "path": "Cargo.toml" }),
            caller: None,
        }
    }

    fn tool_call_result_record(call: &ToolCallRecord) -> ToolResultRecord {
        ToolResultRecord {
            item_id: call.item_id.clone(),
            call_id: call.call_id.clone(),
            name: call.name.clone(),
            kind: call.kind,
        }
    }

    fn assistant_tool_call_history(call: ToolCallRecord) -> Message {
        Message {
            presentation: Default::default(),
            role: MessageRole::Assistant,
            content: MessageContent::text(String::new()),
            reasoning_content: None,
            tool_calls: Some(vec![call]),
            tool_result: None,
            metadata: HashMap::new(),
        }
    }

    fn tool_result_history(record: ToolResultRecord, output: &str) -> Message {
        Message {
            presentation: Default::default(),
            role: MessageRole::Tool,
            content: MessageContent::text(output.to_string()),
            reasoning_content: None,
            tool_calls: None,
            tool_result: Some(record),
            metadata: HashMap::new(),
        }
    }

    fn request_with_tool_history(tool_result: Option<ToolResultRecord>) -> CompletionRequest {
        let call = custom_tool_call_record();
        CompletionRequest::builder()
            .input(context_items(match tool_result {
                Some(record) => vec![
                    assistant_tool_call_history(call),
                    tool_result_history(record, "ok"),
                ],
                None => vec![assistant_tool_call_history(call)],
            }))
            .build()
    }

    fn request_with_function_tool_history(
        tool_result: Option<ToolResultRecord>,
    ) -> CompletionRequest {
        let call = function_tool_call_record();
        CompletionRequest::builder()
            .input(context_items(match tool_result {
                Some(record) => vec![
                    assistant_tool_call_history(call),
                    tool_result_history(record, "ok"),
                ],
                None => vec![assistant_tool_call_history(call)],
            }))
            .build()
    }

    #[test]
    fn responses_replays_program_caller_and_native_items_in_order() {
        let call = ToolCallRecord {
            item_id: "fc_1".to_string(),
            call_id: "call_1".to_string(),
            name: "read_file".to_string(),
            kind: ToolCallKind::Function,
            arguments: serde_json::json!({"path": "README.md"}),
            caller: Some(ToolCallCaller::Program {
                caller_id: "program-1".to_string(),
            }),
        };
        let mut request = request_with_effort("xhigh");
        request.input = vec![
            ModelContextItem::Responses {
                item: ResponsesContextItem {
                    kind: ResponsesContextItemKind::Program,
                    value: serde_json::json!({"type": "program", "id": "program-1"}),
                },
            },
            ModelContextItem::from(assistant_tool_call_history(call.clone())),
            ModelContextItem::from(tool_result_history(
                tool_call_result_record(&call),
                r#"{"content":"ok"}"#,
            )),
            ModelContextItem::Responses {
                item: ResponsesContextItem {
                    kind: ResponsesContextItemKind::ProgramOutput,
                    value: serde_json::json!({
                        "type": "program_output",
                        "id": "program-output-1"
                    }),
                },
            },
        ];

        let body = OpenAiProtocol::responses().build_request_body(&request);

        assert_eq!(body["input"][0]["type"], "program");
        assert_eq!(body["input"][1]["type"], "function_call");
        assert_eq!(body["input"][1]["caller"]["caller_id"], "program-1");
        assert_eq!(body["input"][2]["type"], "function_call_output");
        assert_eq!(body["input"][2]["caller"]["caller_id"], "program-1");
        assert_eq!(body["input"][3]["type"], "program_output");

        let error = OpenAiProtocol::chat()
            .build_request(&request, &ModelInfo::fallback("gpt-5.5"), None)
            .unwrap_err();
        assert!(error.to_string().contains("Responses native items"));
    }

    #[test]
    fn responses_id_only_tool_identity_survives_strict_history_replay() {
        let response = OpenAiProtocol::responses()
            .parse_response(serde_json::json!({
                "model": "gpt-5.5",
                "output": [{
                    "type": "function_call",
                    "id": "fc_1",
                    "name": "read_file",
                    "arguments": "{}"
                }]
            }))
            .unwrap();
        let call = ToolCallRecord {
            item_id: response.tool_calls[0].id.clone(),
            call_id: response.tool_calls[0].call_id.clone(),
            name: "read_file".to_string(),
            kind: ToolCallKind::Function,
            arguments: serde_json::json!({}),
            caller: None,
        };
        let request = CompletionRequest::builder()
            .input(context_items(vec![
                assistant_tool_call_history(call.clone()),
                tool_result_history(tool_call_result_record(&call), "ok"),
            ]))
            .build();

        let body = serde_json::to_value(
            OpenAiProtocol::responses()
                .build_request(&request, &ModelInfo::fallback("gpt-5.5"), None)
                .unwrap(),
        )
        .unwrap();

        assert_eq!(body["input"][0]["call_id"], "fc_1");
        assert_eq!(body["input"][1]["call_id"], "fc_1");
    }

    #[test]
    fn responses_history_replays_custom_tool_call_and_output() {
        let call = custom_tool_call_record();
        let request = request_with_tool_history(Some(tool_call_result_record(&call)));

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
        let call = custom_tool_call_record();
        let request = request_with_tool_history(Some(tool_call_result_record(&call)));

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
        let call = function_tool_call_record();
        let request = request_with_function_tool_history(Some(tool_call_result_record(&call)));

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
    fn tool_result_without_typed_record_fails_request_build() {
        let mut request = request_with_function_tool_history(None);
        request.input.push(ModelContextItem::from(Message {
            presentation: Default::default(),
            role: MessageRole::Tool,
            content: MessageContent::text("ok".to_string()),
            reasoning_content: None,
            tool_calls: None,
            tool_result: None,
            metadata: HashMap::new(),
        }));

        let error = OpenAiProtocol::responses()
            .build_request(&request, &ModelInfo::fallback("gpt-5.5"), None)
            .unwrap_err();

        match error {
            PureError::LlmError(message) => {
                assert!(message.contains("tool result message missing typed tool_result record"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn missing_tool_output_fails_request_build() {
        let request = request_with_function_tool_history(None);

        let error = OpenAiProtocol::responses()
            .build_request(&request, &ModelInfo::fallback("gpt-5.5"), None)
            .unwrap_err();

        match error {
            PureError::LlmError(message) => {
                assert!(message.contains("assistant tool call fc_1 is missing tool output"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn chat_history_with_item_id_call_id_replays_on_both_endpoints() {
        // Chat Completions 解码确定性赋 call_id = item_id；Responses 回放没有
        // missing call_id 路径。
        let call = ToolCallRecord {
            item_id: "fc_1".to_string(),
            call_id: "fc_1".to_string(),
            name: "read_file".to_string(),
            kind: ToolCallKind::Function,
            arguments: serde_json::json!({ "path": "Cargo.toml" }),
            caller: None,
        };
        let request = CompletionRequest::builder()
            .input(context_items(vec![
                assistant_tool_call_history(call.clone()),
                tool_result_history(tool_call_result_record(&call), "ok"),
            ]))
            .build();

        let responses_body = OpenAiProtocol::responses()
            .build_request_body_with_model(&request, &ModelInfo::fallback("gpt-5.5"));
        let chat_body = OpenAiProtocol::chat().build_request_body(&request);

        assert_eq!(
            responses_body["input"][0]["call_id"],
            serde_json::json!("fc_1")
        );
        assert_eq!(
            responses_body["input"][1]["call_id"],
            serde_json::json!("fc_1")
        );
        assert_eq!(
            chat_body["messages"][1]["tool_call_id"],
            serde_json::json!("fc_1")
        );
    }

    #[test]
    fn chat_then_responses_replay_pairs_call_ids_across_protocols() {
        // 第一段：Chat provider 解码工具调用，确定性赋 call_id = item_id。
        let decoded = OpenAiProtocol::chat()
            .parse_response(serde_json::json!({
                "model": "glm-5",
                "choices": [{
                    "message": {
                        "role": "assistant",
                        "tool_calls": [{
                            "id": "chat-call-1",
                            "type": "function",
                            "function": {
                                "name": "read_file",
                                "arguments": "{\"path\":\"Cargo.toml\"}"
                            }
                        }]
                    },
                    "finish_reason": "tool_calls"
                }]
            }))
            .unwrap();
        let decoded_call = &decoded.tool_calls[0];
        assert_eq!(decoded_call.id, "chat-call-1");
        assert_eq!(decoded_call.call_id, "chat-call-1");

        // 第二段：会话保存 typed 记录后切换 Responses provider 继续对话。
        // 记录形状与 pl-core `session::tool_history::tool_call_record` 一致。
        let call = ToolCallRecord {
            item_id: decoded_call.id.clone(),
            call_id: decoded_call.call_id.clone(),
            name: decoded_call.name.clone(),
            kind: ToolCallKind::Function,
            arguments: decoded_call.arguments_for_tool(),
            caller: None,
        };
        let request = CompletionRequest::builder()
            .input(context_items(vec![
                assistant_tool_call_history(call.clone()),
                tool_result_history(tool_call_result_record(&call), "ok"),
            ]))
            .build();

        let body = serde_json::to_value(
            OpenAiProtocol::responses()
                .build_request(&request, &ModelInfo::fallback("gpt-5.5"), None)
                .expect("Responses replay must not hit a missing call_id path"),
        )
        .unwrap();

        assert_eq!(body["input"][0]["type"], "function_call");
        assert_eq!(body["input"][0]["call_id"], "chat-call-1");
        assert_eq!(body["input"][1]["type"], "function_call_output");
        assert_eq!(body["input"][1]["call_id"], "chat-call-1");
        assert_eq!(
            body["input"][0]["call_id"], body["input"][1]["call_id"],
            "assistant function_call 与 function_call_output 必须按 call_id 配对"
        );
    }

    #[test]
    fn responses_then_chat_replay_pairs_tool_call_ids_across_protocols() {
        // 第一段：Responses provider 解码工具调用，保留独立 call_id。
        let decoded = OpenAiProtocol::responses()
            .parse_response(serde_json::json!({
                "model": "gpt-5.5",
                "output": [{
                    "type": "function_call",
                    "id": "fc_1",
                    "call_id": "call_1",
                    "name": "read_file",
                    "arguments": "{\"path\":\"Cargo.toml\"}"
                }]
            }))
            .unwrap();
        let decoded_call = &decoded.tool_calls[0];
        assert_eq!(decoded_call.id, "fc_1");
        assert_eq!(decoded_call.call_id, "call_1");

        // 第二段：切换 Chat provider 继续对话；Chat wire 使用 item_id 配对。
        let call = ToolCallRecord {
            item_id: decoded_call.id.clone(),
            call_id: decoded_call.call_id.clone(),
            name: decoded_call.name.clone(),
            kind: ToolCallKind::Function,
            arguments: decoded_call.arguments_for_tool(),
            caller: None,
        };
        let request = CompletionRequest::builder()
            .input(context_items(vec![
                assistant_tool_call_history(call.clone()),
                tool_result_history(tool_call_result_record(&call), "ok"),
            ]))
            .build();

        let chat_body = OpenAiProtocol::chat().build_request_body(&request);
        let responses_body = serde_json::to_value(
            OpenAiProtocol::responses()
                .build_request(&request, &ModelInfo::fallback("glm-5"), None)
                .expect("Responses replay must keep the provider call_id"),
        )
        .unwrap();

        assert_eq!(chat_body["messages"][0]["tool_calls"][0]["id"], "fc_1");
        assert_eq!(chat_body["messages"][1]["tool_call_id"], "fc_1");
        assert_eq!(responses_body["input"][0]["call_id"], "call_1");
        assert_eq!(responses_body["input"][1]["call_id"], "call_1");
    }
}
