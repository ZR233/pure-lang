use serde::{Deserialize, Serialize};

use crate::StudioUpdate;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstallerLaunchedUpdateState {
    pub(super) revision: u64,
    pub(super) launched_at: i64,
    pub(super) update: StudioUpdate,
}

impl InstallerLaunchedUpdateState {
    pub const fn launched_at(&self) -> i64 {
        self.launched_at
    }

    pub const fn update(&self) -> &StudioUpdate {
        &self.update
    }
}
