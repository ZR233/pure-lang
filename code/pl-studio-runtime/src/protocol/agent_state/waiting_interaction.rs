use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StudioWaitingInteractionAgent {
    turn_id: String,
    interaction_id: String,
}

impl StudioWaitingInteractionAgent {
    pub fn new(turn_id: impl Into<String>, interaction_id: impl Into<String>) -> Self {
        Self {
            turn_id: turn_id.into(),
            interaction_id: interaction_id.into(),
        }
    }

    pub fn turn_id(&self) -> &str {
        &self.turn_id
    }

    pub fn interaction_id(&self) -> &str {
        &self.interaction_id
    }
}
