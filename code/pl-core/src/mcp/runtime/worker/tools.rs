//! MCP 工具定义的过滤、规整与序列化,以及请求超时配置解析。

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use pl_protocol::{PureError, Result};
use rmcp::model::Tool;
use serde_json::{Map, Value};

use crate::config::McpServerConfig;

pub(super) const PROBE_TIMEOUT: Duration = Duration::from_secs(8);

const DEFAULT_TOOL_TIMEOUT: Duration = Duration::from_secs(120);

pub(super) fn filter_tool_definitions(tools: Vec<Tool>, config: &McpServerConfig) -> Vec<Tool> {
    let enabled = config
        .enabled_tools
        .as_ref()
        .map(|names| names.iter().map(String::as_str).collect::<BTreeSet<_>>());
    let disabled = config
        .disabled_tools
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    tools
        .into_iter()
        .filter(|tool| {
            let name = tool.name.as_ref();
            enabled.as_ref().is_none_or(|names| names.contains(name)) && !disabled.contains(name)
        })
        .map(normalize_tool_definition)
        .collect()
}

fn normalize_tool_definition(mut tool: Tool) -> Tool {
    let mut schema = tool.input_schema.as_ref().clone();
    schema
        .entry("type".to_string())
        .or_insert_with(|| Value::String("object".to_string()));
    if schema
        .get("properties")
        .is_none_or(serde_json::Value::is_null)
    {
        schema.insert("properties".to_string(), Value::Object(Map::new()));
    }
    tool.input_schema = Arc::new(schema);
    tool
}

pub(super) fn serialize_optional<T: serde::Serialize>(value: &Option<T>) -> Option<Value> {
    value
        .as_ref()
        .and_then(|value| serde_json::to_value(value).ok())
}

pub(super) fn serialize_resource_result(result: impl serde::Serialize) -> Result<Value> {
    serde_json::to_value(result).map_err(PureError::from)
}

pub(super) fn configured_startup_timeout(seconds: Option<u64>) -> Duration {
    seconds.map(Duration::from_secs).unwrap_or(PROBE_TIMEOUT)
}

pub(super) fn configured_tool_timeout(seconds: Option<u64>) -> Duration {
    seconds
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_TOOL_TIMEOUT)
}
