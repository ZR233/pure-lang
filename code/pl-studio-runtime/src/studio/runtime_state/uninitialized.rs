use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UninitializedStudioRuntime {
    created_at: i64,
}

impl UninitializedStudioRuntime {
    pub(super) fn new(created_at: i64) -> Self {
        Self { created_at }
    }

    pub fn created_at(&self) -> i64 {
        self.created_at
    }
}
