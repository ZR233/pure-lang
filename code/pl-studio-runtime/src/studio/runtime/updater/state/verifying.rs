use serde::{Deserialize, Serialize};

use crate::StudioUpdate;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VerifyingUpdateState {
    pub(super) revision: u64,
    pub(super) updated_at: i64,
    pub(super) update: StudioUpdate,
    pub(super) downloaded: u64,
    pub(super) total: u64,
}

impl VerifyingUpdateState {
    pub const fn update(&self) -> &StudioUpdate {
        &self.update
    }

    pub const fn downloaded(&self) -> u64 {
        self.downloaded
    }

    pub const fn total(&self) -> u64 {
        self.total
    }
}
