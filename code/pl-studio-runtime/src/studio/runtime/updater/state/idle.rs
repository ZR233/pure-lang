use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IdleUpdateState {
    pub(super) revision: u64,
    pub(super) updated_at: i64,
}

impl IdleUpdateState {
    pub(super) const fn new(updated_at: i64) -> Self {
        Self {
            revision: 0,
            updated_at,
        }
    }
}
