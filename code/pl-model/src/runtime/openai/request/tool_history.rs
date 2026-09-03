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
