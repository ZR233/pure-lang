use pl_protocol::StateError;
use serde::{Deserialize, Serialize};

use crate::StudioUpdate;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstallFailedUpdateState {
    pub(super) revision: u64,
    pub(super) failed_at: i64,
    pub(super) update: StudioUpdate,
    pub(super) error: StateError,
}

impl InstallFailedUpdateState {
    pub const fn failed_at(&self) -> i64 {
        self.failed_at
    }

    pub const fn update(&self) -> &StudioUpdate {
        &self.update
    }

    pub const fn error(&self) -> &StateError {
        &self.error
    }
}
