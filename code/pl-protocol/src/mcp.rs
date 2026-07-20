use serde::{Deserialize, Serialize};

/// 无敏感信息的 MCP server 描述。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct McpServerDescriptor {
    pub id: String,
    pub source: String,
    pub transport: String,
    pub endpoint: String,
    pub built_in: bool,
}

/// MCP server 当前可用性的公共投影。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct McpAvailabilityDescriptor {
    pub server: McpServerDescriptor,
    pub availability: String,
    pub message: Option<String>,
    pub last_checked_at: Option<i64>,
    pub tool_count: Option<usize>,
}

/// 某个 runtime generation 的无敏感信息健康快照。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct McpHealthSnapshot {
    pub generation: u64,
    pub servers: Vec<McpAvailabilityDescriptor>,
}
