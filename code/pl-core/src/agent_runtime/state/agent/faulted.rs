use serde::{Deserialize, Serialize};

use crate::agent_runtime::TurnId;
use pl_protocol::StateError;

/// 因内部错误停止工作的 Agent 终态。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FaultedAgentState {
    error: StateError,
    turn_id: Option<TurnId>,
}

impl FaultedAgentState {
    pub fn new(error: StateError, turn_id: Option<TurnId>) -> Self {
        Self { error, turn_id }
    }

    pub fn error(&self) -> &StateError {
        &self.error
    }

    pub fn turn_id(&self) -> Option<&TurnId> {
        self.turn_id.as_ref()
    }
}
