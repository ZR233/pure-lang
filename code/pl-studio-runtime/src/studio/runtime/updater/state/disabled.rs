use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DisabledUpdateState {
    pub(super) revision: u64,
    pub(super) updated_at: i64,
}
