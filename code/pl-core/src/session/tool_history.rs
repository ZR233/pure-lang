use std::collections::HashSet;

use pl_model::{ToolCall, ToolCallPayload};
use pl_protocol::{
    Message, MessageContent, MessageRole, ToolCallKind, ToolCallRecord, ToolResultRecord,
};

pub(super) fn messages_from_items(items: &[pl_protocol::ModelContextItem]) -> Vec<Message> {
    items
        .iter()
        .filter_map(pl_protocol::ModelContextItem::as_message)
        .cloned()
        .collect()
}

/// 把 provider 解码出的 `ToolCall` 投影为会话持久化的 typed 记录。
pub fn tool_call_record(call: &ToolCall) -> ToolCallRecord {
    ToolCallRecord {
        item_id: call.id.clone(),
        call_id: call.call_id.clone(),
        name: call.name.clone(),
        kind: call.kind(),
        arguments: record_arguments(call),
        caller: call.caller.clone(),
    }
}

/// 记录中的参数形态：
///
/// - function 且参数合法：解析后的 JSON；
/// - function 且参数非法：保留原始文本的字符串字面量；
/// - custom：输入文本的字符串字面量。
fn record_arguments(call: &ToolCall) -> serde_json::Value {
    if let Some(invalid) = &call.invalid_arguments {
        return serde_json::Value::String(invalid.raw.clone());
    }
    match &call.payload {
        ToolCallPayload::Function { arguments } => arguments.clone(),
        ToolCallPayload::Custom { input } => serde_json::Value::String(input.clone()),
    }
}

/// 构造包含 typed 工具调用的 assistant 历史消息。
///
/// 宿主测试或迁移工具需要手工构造历史时，应复用该 helper，而不是手写
/// `tool_calls` 记录。生产 turn loop 仍应优先通过 `AgentSession` 记录模型
/// 返回的真实 `ToolCall`。
pub fn tool_call_history_message(
    call_id: String,
    tool_name: String,
    raw_arguments: String,
) -> Message {
    let arguments =
        serde_json::from_str(&raw_arguments).unwrap_or(serde_json::Value::String(raw_arguments));
    Message {
        role: MessageRole::Assistant,
        content: MessageContent::Text(String::new()),
        reasoning_content: None,
        tool_calls: Some(vec![ToolCallRecord {
            item_id: call_id.clone(),
            call_id,
            name: tool_name,
            kind: ToolCallKind::Function,
            arguments,
            caller: None,
        }]),
        tool_result: None,
        metadata: Default::default(),
    }
}

/// 构造包含 typed 配对记录的 tool result 历史消息。
pub fn tool_result_history_message(call_id: String, tool_name: String, output: String) -> Message {
    Message {
        role: MessageRole::Tool,
        content: MessageContent::Text(output),
        reasoning_content: None,
        tool_calls: None,
        tool_result: Some(ToolResultRecord {
            item_id: call_id.clone(),
            call_id,
            name: tool_name,
            kind: ToolCallKind::Function,
        }),
        metadata: Default::default(),
    }
}

/// 修复不完整的工具调用历史。
///
/// 宿主恢复中断 turn 时，历史里可能保留 assistant tool call，但缺少对应 tool
/// result。模型协议要求每个 tool call 都有结果；该函数会在下一条非 tool 消息前
/// 插入 synthetic interrupted tool result，并返回历史是否发生变化。
pub fn repair_incomplete_tool_history(history: &mut Vec<Message>) -> bool {
    let mut insertions: Vec<(usize, Vec<Message>)> = Vec::new();
    let mut i = 0;
    while i < history.len() {
        let mut pending_calls = Vec::new();
        while i < history.len() {
            if history[i]
                .tool_calls
                .as_ref()
                .is_some_and(|calls| !calls.is_empty())
            {
                pending_calls.extend(history[i].tool_calls.iter().flatten().cloned());
                i += 1;
            } else {
                break;
            }
        }
        if pending_calls.is_empty() {
            i += 1;
            continue;
        }

        let mut answered = HashSet::new();
        while i < history.len() {
            if history[i].role == MessageRole::Tool
                && let Some(record) = &history[i].tool_result
                && pending_calls
                    .iter()
                    .any(|call| call.call_id == record.call_id)
            {
                answered.insert(record.call_id.clone());
                i += 1;
                continue;
            }
            break;
        }

        let missing_outputs = pending_calls
            .into_iter()
            .filter(|call| !answered.contains(&call.call_id))
            .map(interrupted_tool_result_message)
            .collect::<Vec<_>>();
        if !missing_outputs.is_empty() {
            insertions.push((i, missing_outputs));
        }
    }

    let changed = !insertions.is_empty();
    for (pos, items) in insertions.into_iter().rev() {
        for item in items.into_iter().rev() {
            history.insert(pos, item);
        }
    }
    changed
}

fn interrupted_tool_result_message(call: ToolCallRecord) -> Message {
    tool_result_message(
        ToolResultRecord {
            item_id: call.item_id,
            call_id: call.call_id,
            name: call.name,
            kind: call.kind,
        },
        "error: tool execution interrupted",
    )
}

/// 构造 canonical tool result 消息；metadata 保持为空。
pub(super) fn tool_result_message(record: ToolResultRecord, result: &str) -> Message {
    Message {
        role: MessageRole::Tool,
        content: MessageContent::Text(result.to_string()),
        reasoning_content: None,
        tool_calls: None,
        tool_result: Some(record),
        metadata: Default::default(),
    }
}
