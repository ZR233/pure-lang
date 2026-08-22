use serde::{Deserialize, Serialize};

/// 尚未加载、没有可用值的资源。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UninitializedResource {
    revision: u64,
    updated_at: i64,
}

impl UninitializedResource {
    pub(super) fn new(updated_at: i64) -> Self {
        Self {
            revision: 0,
            updated_at,
        }
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn updated_at(&self) -> i64 {
        self.updated_at
    }
}
