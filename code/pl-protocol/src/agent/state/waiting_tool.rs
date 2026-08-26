use serde::{Deserialize, Serialize};

use crate::TurnId;

/// 等待工具或 child Agent 返回的 Agent。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WaitingToolAgentState {
    turn_id: TurnId,
}

impl WaitingToolAgentState {
    pub fn new(turn_id: TurnId) -> Self {
        Self { turn_id }
    }

    pub fn turn_id(&self) -> &TurnId {
        &self.turn_id
    }
}
