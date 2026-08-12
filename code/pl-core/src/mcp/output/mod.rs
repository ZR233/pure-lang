use std::path::PathBuf;

use pl_protocol::{PureError, Result};
use rmcp::model::CallToolResult;
use serde_json::Value;

use crate::tool::{OutputTruncation, ToolOutput, ToolRuntimeEvent};

/// 把 rmcp typed result 转成统一工具输出，同时保留完整审计 payload。
pub(super) fn call_tool_result_to_output(
    server_id: &str,
    raw_tool_name: &str,
    result: CallToolResult,
) -> Result<ToolOutput> {
    let content = result
        .content
        .iter()
        .map(|content| {
            serde_json::to_value(content).map_err(|error| PureError::ToolExecutionFailed {
                tool: raw_tool_name.to_string(),
                error: format!("failed to serialize MCP content: {error}"),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let model_content = format_mcp_content(&content);
    let is_error = result.is_error.unwrap_or(false);
    let result_value =
        serde_json::to_value(&result).map_err(|error| PureError::ToolExecutionFailed {
            tool: raw_tool_name.to_string(),
            error: format!("failed to serialize MCP result metadata: {error}"),
        })?;
    let audit = serde_json::json!({
        "type": "mcpCallResult",
        "server": server_id,
        "tool": raw_tool_name,
        "result": result_value,
    });
    let description = if is_error {
        format!("Tool execution error: {model_content}")
    } else {
        model_content
    };
    let mut runtime_events = vec![ToolRuntimeEvent::AuditMetadata { metadata: audit }];
    if is_error {
        runtime_events.push(ToolRuntimeEvent::ExecutionFailed);
    }
    Ok(ToolOutput {
        description,
        truncated: OutputTruncation::empty(),
        output_file: PathBuf::new(),
        exit_code: is_error.then_some(1),
        timed_out: false,
        runtime_events,
    })
}

fn format_mcp_content(content: &[Value]) -> String {
    content
        .iter()
        .map(format_mcp_content_part)
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_mcp_content_part(content: &Value) -> String {
    let Some(object) = content.as_object() else {
        return compact_json(content);
    };
    match object.get("type").and_then(Value::as_str) {
        Some("text") => object
            .get("text")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| compact_json(content)),
        Some("json") => object
            .get("json")
            .map(compact_json)
            .unwrap_or_else(|| compact_json(content)),
        _ => compact_json(content),
    }
}

fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| value.to_string())
}
