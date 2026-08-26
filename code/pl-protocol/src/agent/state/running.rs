use serde::{Deserialize, Serialize};

use crate::TurnId;

/// 正在执行模型或本地编排的 Agent。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunningAgentState {
    turn_id: TurnId,
}

impl RunningAgentState {
    pub fn new(turn_id: TurnId) -> Self {
        Self { turn_id }
    }

    pub fn turn_id(&self) -> &TurnId {
        &self.turn_id
    }
}
