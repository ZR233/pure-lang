use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StoppedStudioRuntime {
    stopped_at: i64,
}

impl StoppedStudioRuntime {
    pub(super) fn new(stopped_at: i64) -> Self {
        Self { stopped_at }
    }

    pub fn stopped_at(&self) -> i64 {
        self.stopped_at
    }
}
