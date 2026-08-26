use serde::{Deserialize, Serialize};

/// 已完成资源释放的 Agent 终态。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ClosedAgentState;

impl ClosedAgentState {
    pub fn new() -> Self {
        Self
    }
}
