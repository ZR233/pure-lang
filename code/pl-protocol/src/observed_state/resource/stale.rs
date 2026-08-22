use serde::{Deserialize, Serialize};

/// 已明确失效但仍可展示 last-known value 的资源。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StaleResource<T> {
    revision: u64,
    stale_at: i64,
    last_checked_at: Option<i64>,
    value: T,
}

impl<T> StaleResource<T> {
    pub(super) fn new(
        revision: u64,
        stale_at: i64,
        last_checked_at: Option<i64>,
        value: T,
    ) -> Self {
        Self {
            revision,
            stale_at,
            last_checked_at,
            value,
        }
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn stale_at(&self) -> i64 {
        self.stale_at
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
