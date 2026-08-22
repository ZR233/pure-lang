use std::collections::BTreeSet;

use pl_protocol::{ModelContextItem, ResponsesContextItem, ResponsesContextItemKind, Result};
use pl_trace::TracePartStatus;

use crate::session::AgentSession;
use crate::tool::{ClientToolSearchResolution, ToolInventory};
use crate::trace::TraceRecorder;
use crate::turn::{BudgetTracker, TurnOptions};

/// 本轮 provider 响应中的 client tool search 调用集合。
pub(super) struct ClientToolSearchBatch {
    /// 由 function call 合成的有序 `tool_search_call` 上下文项。
    call_items: Vec<ResponsesContextItem>,
    pub(super) resolution: ClientToolSearchResolution,
    /// 每个调用（原生与合成）的模型原始 arguments 文本，按 call_id 配对。
    arguments_by_call: std::collections::BTreeMap<String, String>,
}

/// 汇总客户端 tool search 调用。
///
/// `tool_search` 以普通 function 工具形式发送，模型调用先于普通 dispatch 被拦截，
/// 在冻结 catalog 上检索并产出配对的 `tool_search_output`；provider 原生返回的
/// `tool_search_call`（execution=client）项同样处理。session 中已有配对 output 的
/// call_id 不重复解析，保证 HTTP/WS/恢复回放幂等。
pub(super) fn collect_client_tool_search(
    tool_calls: &mut Vec<pl_model::ToolCall>,
    response_items: &[ResponsesContextItem],
    inventory: &ToolInventory,
    session: &AgentSession,
) -> Result<ClientToolSearchBatch> {
    let paired = paired_tool_search_call_ids(session);
    let mut calls = Vec::new();
    let mut call_items = Vec::new();
    let mut arguments_by_call = std::collections::BTreeMap::new();
    for item in response_items {
        if item.kind != ResponsesContextItemKind::ToolSearchCall {
            continue;
        }
        if item
            .value
            .get("execution")
            .and_then(serde_json::Value::as_str)
            != Some("client")
        {
            continue;
        }
        let call_id = item
            .value
            .get("call_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string();
        if !call_id.is_empty() && paired.contains(&call_id) {
            continue;
        }
        if !call_id.is_empty() {
            arguments_by_call.insert(call_id.clone(), arguments_text(item.value.get("arguments")));
        }
        calls.push(item.clone());
    }
    if inventory.catalog().is_some() {
        tool_calls.retain(|tool_call| {
            if tool_call.name != "tool_search" {
                return true;
            }
            let call_id = tool_call.call_id.clone();
            if call_id.is_empty() || paired.contains(&call_id) {
                return false;
            }
            let arguments = tool_call.payload_text();
            if !call_id.is_empty() {
                arguments_by_call.insert(call_id.clone(), arguments);
            }
            let arguments_wire = match &tool_call.payload {
                pl_model::ToolCallPayload::Function { arguments } => arguments.clone(),
                pl_model::ToolCallPayload::Custom { input } => {
                    serde_json::json!({ "input": input })
                }
            };
            let call_item = ResponsesContextItem {
                kind: ResponsesContextItemKind::ToolSearchCall,
                value: serde_json::json!({
                    "type": "tool_search_call",
                    "call_id": call_id,
                    "execution": "client",
                    "arguments": arguments_wire,
                }),
            };
            calls.push(call_item.clone());
            call_items.push(call_item);
            false
        });
    }
    let resolution = if calls.is_empty() {
        ClientToolSearchResolution::default()
    } else {
        inventory.resolve_client_search_calls(&calls)?
    };
    Ok(ClientToolSearchBatch {
        call_items,
        resolution,
        arguments_by_call,
    })
}

/// session 中已存在 `tool_search_output` 的 call_id 集合。
fn paired_tool_search_call_ids(session: &AgentSession) -> BTreeSet<String> {
    session
        .items()
        .iter()
        .filter_map(|item| match item {
            ModelContextItem::Responses {
                item:
                    ResponsesContextItem {
                        kind: ResponsesContextItemKind::ToolSearchOutput,
                        value,
                    },
            } => value
                .get("call_id")
                .and_then(serde_json::Value::as_str)
                .map(ToString::to_string),
            _ => None,
        })
        .collect()
}

/// 把 client tool search 的 call/output 项写入 canonical context 并记录指标。
pub(super) fn apply_client_tool_search(
    session: &mut AgentSession,
    batch: &ClientToolSearchBatch,
    budget_tracker: &mut BudgetTracker,
    options: &TurnOptions,
    orchestration: &mut pl_protocol::InferenceOrchestrationMetrics,
) {
    let output_count = batch.resolution.outputs.len();
    for _ in 0..output_count {
        options.apply_budget_refresh(budget_tracker);
        budget_tracker.record_tool_call("tool_search");
    }
    let output_texts = batch
        .resolution
        .outputs
        .iter()
        .map(|item| item.value.to_string())
        .collect::<Vec<_>>();
    let estimated_tokens =
        crate::tool::estimate_tool_result_tokens(output_texts.iter().map(String::as_str));
    session.push_responses_context_items(batch.call_items.clone());
    session.push_responses_context_items(batch.resolution.outputs.clone());
    orchestration.tool_search_calls = orchestration
        .tool_search_calls
        .saturating_add(output_count as u64);
    orchestration.tool_search_loaded_tools = orchestration
        .tool_search_loaded_tools
        .saturating_add(batch.resolution.loaded_tool_count);
    orchestration.tool_result_estimated_tokens = estimated_tokens;
}

/// 原生 `tool_search_call` 的 arguments wire 值转为模型原始 JSON 文本。
fn arguments_text(value: Option<&serde_json::Value>) -> String {
    match value {
        Some(serde_json::Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

/// 为拦截的 client tool search 调用记录 toolCall timeline Item。
///
/// 拦截路径不经过常规 tool dispatch，因此在此显式补齐每个 `tool_search` 调用
/// 的 toolCall Item；item result 是面向展示的结构化摘要（query、loaded 工具名
/// 与 namespace），canonical context 仍由 `tool_search_call` / `tool_search_output`
/// 承载，两个事实层分离。
pub(super) fn record_client_tool_search_items(
    recorder: &mut TraceRecorder,
    turn_id: &str,
    batch: &ClientToolSearchBatch,
) {
    for summary in &batch.resolution.summaries {
        let arguments = batch
            .arguments_by_call
            .get(&summary.call_id)
            .cloned()
            .unwrap_or_default();
        let mut item = recorder.tool_item(
            turn_id,
            &super::super::tool_dispatch::namespaced_tool_trace_part_id(turn_id, &summary.call_id),
            "tool_search".to_string(),
            arguments,
            Some(summary.call_id.clone()),
            None,
        );
        if let Some(tool) = &mut item.tool {
            tool.result = Some(tool_search_summary_result(summary));
        }
        item.status = TracePartStatus::Completed;
        recorder.start_item(item.clone());
        recorder.complete_item(item);
    }
}

/// 生成 tool_search toolCall Item 的结构化 result 摘要（JSON 字符串）。
fn tool_search_summary_result(summary: &crate::tool::ClientToolSearchCallSummary) -> String {
    let tools = summary
        .groups
        .iter()
        .flat_map(|(namespace, names)| {
            names
                .iter()
                .map(move |name| serde_json::json!({ "namespace": namespace, "name": name }))
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "type": "tool_search",
        "query": summary.query,
        "loadedToolCount": tools.len(),
        "tools": tools,
    })
    .to_string()
}
