use std::collections::{HashMap, HashSet};

use pl_protocol::{
    Message, MessageContent, MessageRole, ModelContextItem, TOOL_CALLS_METADATA_KEY,
    ToolCallHistoryMetadata, ToolCallKind, ToolResultMetadata,
};

pub(super) fn messages_from_items(items: &[ModelContextItem]) -> Vec<Message> {
    items
        .iter()
        .filter_map(ModelContextItem::as_message)
        .cloned()
        .collect()
}

/// 构造包含 assistant tool_calls metadata 的历史消息。
///
/// 宿主测试或迁移工具需要手工构造历史时，应复用该 helper，而不是直接拼
/// `tool_calls` metadata JSON。生产 turn loop 仍应优先通过 `AgentSession`
/// 记录模型返回的真实 `ToolCall`。
pub fn tool_call_history_message(
    call_id: String,
    tool_name: String,
    raw_arguments: String,
) -> Message {
    let arguments =
        serde_json::from_str(&raw_arguments).unwrap_or(serde_json::Value::String(raw_arguments));
    let tool_calls = serde_json::json!([{
        "id": call_id,
        "name": tool_name,
        "payload": {
            "kind": "function",
            "arguments": arguments
        },
        "call_id": call_id
    }])
    .to_string();
    let mut metadata = HashMap::new();
    ToolCallHistoryMetadata::new(tool_calls).insert_into(&mut metadata);
    Message {
        role: MessageRole::Assistant,
        content: MessageContent::Text(String::new()),
        reasoning_content: None,
        metadata,
    }
}

/// 构造包含 tool result metadata 的历史消息。
///
/// 该函数集中维护模型历史里工具结果的 metadata 形状，避免宿主产品在测试或
/// 历史修复场景复制 pl-core 的协议细节。
pub fn tool_result_history_message(
    call_id: String,
    tool_name: String,
    raw_arguments: String,
    output: String,
) -> Message {
    let mut metadata = HashMap::new();
    ToolResultMetadata::new(
        call_id,
        None,
        tool_name,
        ToolCallKind::Function,
        raw_arguments,
    )
    .insert_into(&mut metadata);
    Message {
        role: MessageRole::Tool,
        content: MessageContent::Text(output),
        reasoning_content: None,
        metadata,
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
            if history[i].metadata.contains_key(TOOL_CALLS_METADATA_KEY) {
                pending_calls.extend(tool_calls(&history[i]));
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
                && let Ok(metadata) = ToolResultMetadata::from_metadata(&history[i].metadata)
                && pending_calls
                    .iter()
                    .any(|call| call.id == metadata.tool_call_id)
            {
                answered.insert(metadata.tool_call_id);
                i += 1;
                continue;
            }
            break;
        }

        let missing_outputs = pending_calls
            .into_iter()
            .filter(|call| !answered.contains(&call.id))
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingToolCall {
    id: String,
    call_id: Option<String>,
    name: String,
    kind: ToolCallKind,
    arguments: String,
}

fn tool_calls(message: &Message) -> Vec<PendingToolCall> {
    ToolCallHistoryMetadata::from_metadata(&message.metadata)
        .and_then(|metadata| {
            serde_json::from_str::<serde_json::Value>(&metadata.tool_calls_json).ok()
        })
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| {
            let id = item
                .get("id")
                .or_else(|| item.get("call_id"))
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned)?;
            let call_id = item
                .get("call_id")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned);
            let name = item
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string();
            let payload = item.get("payload");
            let kind = payload
                .and_then(|payload| payload.get("kind"))
                .and_then(serde_json::Value::as_str)
                .map(tool_call_kind_from_str)
                .unwrap_or(ToolCallKind::Function);
            let arguments = payload
                .and_then(|payload| payload.get("arguments"))
                .or_else(|| item.get("arguments"))
                .map(tool_call_arguments)
                .unwrap_or_else(|| "{}".to_string());
            Some(PendingToolCall {
                id,
                call_id,
                name,
                kind,
                arguments,
            })
        })
        .collect()
}

fn interrupted_tool_result_message(call: PendingToolCall) -> Message {
    let mut metadata = HashMap::new();
    ToolResultMetadata::new(call.id, call.call_id, call.name, call.kind, call.arguments)
        .insert_into(&mut metadata);
    Message {
        role: MessageRole::Tool,
        content: MessageContent::Text("error: tool execution interrupted".to_string()),
        reasoning_content: None,
        metadata,
    }
}

fn tool_call_kind_from_str(value: &str) -> ToolCallKind {
    match value {
        "custom" => ToolCallKind::Custom,
        "function" => ToolCallKind::Function,
        _ => ToolCallKind::Function,
    }
}

fn tool_call_arguments(value: &serde_json::Value) -> String {
    value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string())
}
