use serde::{Deserialize, Serialize};

use crate::TurnId;

/// 等待指定用户 Interaction 的 Agent。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WaitingInteractionAgentState {
    turn_id: TurnId,
    interaction_id: String,
}

impl WaitingInteractionAgentState {
    pub fn new(turn_id: TurnId, interaction_id: String) -> Self {
        Self {
            turn_id,
            interaction_id,
        }
    }

    pub fn turn_id(&self) -> &TurnId {
        &self.turn_id
    }

    pub fn interaction_id(&self) -> &str {
        &self.interaction_id
    }
}
