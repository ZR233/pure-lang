use serde::{Deserialize, Serialize};

use crate::agent_runtime::TurnId;

/// 正在收束取消请求的 Agent。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CancellingAgentState {
    turn_id: TurnId,
}

impl CancellingAgentState {
    pub fn new(turn_id: TurnId) -> Self {
        Self { turn_id }
    }

    pub fn turn_id(&self) -> &TurnId {
        &self.turn_id
    }
}
