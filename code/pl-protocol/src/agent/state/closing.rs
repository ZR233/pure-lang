use serde::{Deserialize, Serialize};

/// 已拒绝新工作、正在释放资源的 Agent。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ClosingAgentState;

impl ClosingAgentState {
    pub fn new() -> Self {
        Self
    }
}
