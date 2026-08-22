use serde::{Deserialize, Serialize};

use crate::{StateError, StateOperation};

/// 刷新失败但仍保留可展示 last-known value 的资源。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DegradedResource<T> {
    revision: u64,
    failed_at: i64,
    last_checked_at: Option<i64>,
    operation: StateOperation,
    error: StateError,
    value: T,
}

impl<T> DegradedResource<T> {
    pub(super) fn new(
        revision: u64,
        failed_at: i64,
        last_checked_at: Option<i64>,
        operation: StateOperation,
        error: StateError,
        value: T,
    ) -> Self {
        Self {
            revision,
            failed_at,
            last_checked_at,
            operation,
            error,
            value,
        }
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn failed_at(&self) -> i64 {
        self.failed_at
    }

    pub fn last_checked_at(&self) -> Option<i64> {
        self.last_checked_at
    }

    pub fn operation(&self) -> StateOperation {
        self.operation
    }

    pub fn error(&self) -> &StateError {
        &self.error
    }

    pub fn value(&self) -> &T {
        &self.value
    }

    pub(super) fn into_value(self) -> T {
        self.value
    }
}
