use serde::{Deserialize, Serialize};

/// 已停止且不再暴露旧值的资源。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StoppedResource {
    revision: u64,
    stopped_at: i64,
}

impl StoppedResource {
    pub(super) fn new(revision: u64, stopped_at: i64) -> Self {
        Self {
            revision,
            stopped_at,
        }
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn stopped_at(&self) -> i64 {
        self.stopped_at
    }
}
