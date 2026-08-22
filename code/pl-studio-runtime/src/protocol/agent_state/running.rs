use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StudioRunningAgent {
    turn_id: String,
}

impl StudioRunningAgent {
    pub fn new(turn_id: impl Into<String>) -> Self {
        Self {
            turn_id: turn_id.into(),
        }
    }

    pub fn turn_id(&self) -> &str {
        &self.turn_id
    }
}
