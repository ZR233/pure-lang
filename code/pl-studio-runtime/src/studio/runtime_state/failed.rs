use pl_protocol::StateError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FailedStudioRuntime {
    failed_at: i64,
    error: StateError,
}

impl FailedStudioRuntime {
    pub(super) fn new(failed_at: i64, error: StateError) -> Self {
        Self { failed_at, error }
    }

    pub fn failed_at(&self) -> i64 {
        self.failed_at
    }

    pub fn error(&self) -> &StateError {
        &self.error
    }
}
