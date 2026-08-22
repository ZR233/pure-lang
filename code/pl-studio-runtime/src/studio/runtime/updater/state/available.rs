use serde::{Deserialize, Serialize};

use crate::StudioUpdate;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AvailableUpdateState {
    pub(super) revision: u64,
    pub(super) checked_at: i64,
    pub(super) update: StudioUpdate,
}

impl AvailableUpdateState {
    pub const fn checked_at(&self) -> i64 {
        self.checked_at
    }

    pub const fn update(&self) -> &StudioUpdate {
        &self.update
    }
}
