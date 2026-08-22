use pl_protocol::StateError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LspUnavailable {
    checked_at: i64,
    error: StateError,
}

impl LspUnavailable {
    pub fn new(checked_at: i64, error: StateError) -> Self {
        Self { checked_at, error }
    }

    pub fn checked_at(&self) -> i64 {
        self.checked_at
    }

    pub fn error(&self) -> &StateError {
        &self.error
    }
}
