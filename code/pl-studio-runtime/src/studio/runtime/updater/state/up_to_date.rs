use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpToDateUpdateState {
    pub(super) revision: u64,
    pub(super) checked_at: i64,
}

impl UpToDateUpdateState {
    pub const fn checked_at(&self) -> i64 {
        self.checked_at
    }
}
