use pl_protocol::StateError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StudioFaultedAgent {
    error: StateError,
    diagnostic_turn_id: Option<String>,
}

impl StudioFaultedAgent {
    pub fn new(error: StateError, diagnostic_turn_id: Option<String>) -> Self {
        Self {
            error,
            diagnostic_turn_id,
        }
    }

    pub fn error(&self) -> &StateError {
        &self.error
    }

    pub fn diagnostic_turn_id(&self) -> Option<&str> {
        self.diagnostic_turn_id.as_deref()
    }
}
