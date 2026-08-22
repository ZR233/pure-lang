use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpAvailable {
    checked_at: i64,
    tool_count: u64,
}

impl McpAvailable {
    pub fn new(checked_at: i64, tool_count: u64) -> Self {
        Self {
            checked_at,
            tool_count,
        }
    }

    pub fn checked_at(&self) -> i64 {
        self.checked_at
    }

    pub fn tool_count(&self) -> u64 {
        self.tool_count
    }
}
