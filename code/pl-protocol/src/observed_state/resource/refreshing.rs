use serde::{Deserialize, Serialize};

use crate::StateOperation;

/// 刷新中，并保留可展示的 last-known value。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RefreshingResource<T> {
    revision: u64,
    operation: StateOperation,
    operation_id: String,
    started_at: i64,
    last_checked_at: Option<i64>,
    value: T,
}

impl<T> RefreshingResource<T> {
    pub(super) fn new(
        revision: u64,
        operation: StateOperation,
        operation_id: String,
        started_at: i64,
        last_checked_at: Option<i64>,
        value: T,
    ) -> Self {
        Self {
            revision,
            operation,
            operation_id,
            started_at,
            last_checked_at,
            value,
        }
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn operation(&self) -> StateOperation {
        self.operation
    }

    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }

    pub fn started_at(&self) -> i64 {
        self.started_at
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
