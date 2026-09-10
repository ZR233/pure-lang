/// MCP server 的运行时可用状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpAvailabilityKind {
    Checking,
    Available,
    Unavailable,
    Disabled,
    MissingCredential,
}

impl McpAvailabilityKind {
    pub fn as_str(self) -> &'static str {
        match self {
            McpAvailabilityKind::Checking => "checking",
            McpAvailabilityKind::Available => "available",
            McpAvailabilityKind::Unavailable => "unavailable",
            McpAvailabilityKind::Disabled => "disabled",
            McpAvailabilityKind::MissingCredential => "missingCredential",
        }
    }
}

/// 产品投影层展示用的 MCP availability 快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpAvailabilitySnapshot {
    pub server_id: String,
    pub availability_kind: McpAvailabilityKind,
    pub availability_message: Option<String>,
    pub last_checked_at: Option<i64>,
    pub tool_count: Option<usize>,
}
