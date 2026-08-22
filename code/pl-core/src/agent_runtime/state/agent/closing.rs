use serde::{Deserialize, Serialize};

/// 已拒绝新工作、正在释放资源的 Agent。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClosingAgentState;

impl ClosingAgentState {
    pub fn new() -> Self {
        Self
    }
}
