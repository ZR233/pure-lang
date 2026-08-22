use serde::{Deserialize, Serialize};

/// 已就绪且与 desired state 一致的资源。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadyResource<T> {
    revision: u64,
    updated_at: i64,
    last_checked_at: Option<i64>,
    value: T,
}

impl<T> ReadyResource<T> {
    pub(super) fn new(revision: u64, updated_at: i64, value: T) -> Self {
        Self::new_observed(revision, updated_at, None, value)
    }

    pub(super) fn new_observed(
        revision: u64,
        updated_at: i64,
        last_checked_at: Option<i64>,
        value: T,
    ) -> Self {
        Self {
            revision,
            updated_at,
            last_checked_at,
            value,
        }
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn updated_at(&self) -> i64 {
        self.updated_at
    }

    pub fn last_checked_at(&self) -> Option<i64> {
        self.last_checked_at
    }

    pub fn value(&self) -> &T {
        &self.value
    }

    pub(super) fn into_value(self) -> T {
        self.value
    }
}
