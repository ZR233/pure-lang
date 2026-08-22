use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CompletedCleanup {
    operation_id: String,
    completed_at: i64,
}

impl CompletedCleanup {
    pub(crate) fn new(operation_id: String, completed_at: i64) -> Self {
        Self {
            operation_id,
            completed_at,
        }
    }
    pub(crate) fn operation_id(&self) -> &str {
        &self.operation_id
    }
    pub(crate) const fn completed_at(&self) -> i64 {
        self.completed_at
    }
}
