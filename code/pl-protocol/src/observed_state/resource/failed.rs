use serde::{Deserialize, Serialize};

use crate::{StateError, StateOperation};

/// 加载失败且没有可用值的资源。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FailedResource {
    revision: u64,
    failed_at: i64,
    operation: StateOperation,
    error: StateError,
}

impl FailedResource {
    pub(super) fn new(
        revision: u64,
        failed_at: i64,
        operation: StateOperation,
        error: StateError,
    ) -> Self {
        Self {
            revision,
            failed_at,
            operation,
            error,
        }
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn failed_at(&self) -> i64 {
        self.failed_at
    }

    pub fn operation(&self) -> StateOperation {
        self.operation
    }

    pub fn error(&self) -> &StateError {
        &self.error
    }
}
