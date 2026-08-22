use serde::{Deserialize, Serialize};

use crate::StateOperation;

/// 首次加载中；此阶段没有可展示的旧值。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LoadingResource {
    revision: u64,
    operation: StateOperation,
    operation_id: String,
    started_at: i64,
}

impl LoadingResource {
    pub(super) fn new(
        revision: u64,
        operation: StateOperation,
        operation_id: String,
        started_at: i64,
    ) -> Self {
        Self {
            revision,
            operation,
            operation_id,
            started_at,
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
}
