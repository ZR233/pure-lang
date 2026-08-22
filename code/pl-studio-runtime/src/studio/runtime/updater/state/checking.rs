use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CheckingUpdateState {
    pub(super) revision: u64,
    pub(super) operation_id: String,
    pub(super) started_at: i64,
}

impl CheckingUpdateState {
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }

    pub const fn started_at(&self) -> i64 {
        self.started_at
    }
}
