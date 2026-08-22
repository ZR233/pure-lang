use serde::{Deserialize, Serialize};

use crate::agent_runtime::TurnId;

/// 已确定下一 Turn、等待启动的 Agent。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QueuedAgentState {
    turn_id: TurnId,
}

impl QueuedAgentState {
    pub fn new(turn_id: TurnId) -> Self {
        Self { turn_id }
    }

    pub fn turn_id(&self) -> &TurnId {
        &self.turn_id
    }
}
