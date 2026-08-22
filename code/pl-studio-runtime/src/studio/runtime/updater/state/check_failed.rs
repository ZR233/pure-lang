use pl_protocol::StateError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CheckFailedUpdateState {
    pub(super) revision: u64,
    pub(super) failed_at: i64,
    pub(super) error: StateError,
}

impl CheckFailedUpdateState {
    pub const fn failed_at(&self) -> i64 {
        self.failed_at
    }

    pub const fn error(&self) -> &StateError {
        &self.error
    }
}
