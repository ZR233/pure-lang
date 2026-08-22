use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ShuttingDownStudioRuntime {
    started_at: i64,
}

impl ShuttingDownStudioRuntime {
    pub(super) fn new(started_at: i64) -> Self {
        Self { started_at }
    }

    pub fn started_at(&self) -> i64 {
        self.started_at
    }
}
