use serde::{Deserialize, Serialize};

/// 没有 active 或 queued Turn 的 Agent。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct IdleAgentState;

impl IdleAgentState {
    pub fn new() -> Self {
        Self
    }
}
