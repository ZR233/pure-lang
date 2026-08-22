use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadyStudioRuntime {
    ready_at: i64,
}

impl ReadyStudioRuntime {
    pub(super) fn new(ready_at: i64) -> Self {
        Self { ready_at }
    }

    pub fn ready_at(&self) -> i64 {
        self.ready_at
    }
}
