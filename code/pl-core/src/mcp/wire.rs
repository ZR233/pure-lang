use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct JsonRpcRequest<'a> {
    pub(super) jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) id: Option<u64>,
    pub(super) method: &'a str,
    pub(super) params: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct JsonRpcResponse {
    pub(super) id: Option<u64>,
    pub(super) result: Option<Value>,
    pub(super) error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct JsonRpcError {
    pub(super) code: i64,
    pub(super) message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct McpListToolsResult {
    #[serde(default)]
    pub(super) tools: Vec<McpToolDefinition>,
    pub(super) next_cursor: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct McpToolDefinition {
    pub(super) name: String,
    pub(super) description: Option<String>,
    #[serde(default = "default_input_schema")]
    pub(super) input_schema: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct McpCallToolResult {
    #[serde(default)]
    pub(super) content: Vec<Value>,
    #[serde(default)]
    pub(super) is_error: bool,
}

pub(super) fn default_input_schema() -> Value {
    let mut map = Map::new();
    map.insert("type".to_string(), Value::String("object".to_string()));
    map.insert("properties".to_string(), Value::Object(Map::new()));
    Value::Object(map)
}
