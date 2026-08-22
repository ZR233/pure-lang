use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AttemptingCleanup {
    operation_id: String,
    started_at: i64,
}

impl AttemptingCleanup {
    pub(crate) fn new(operation_id: String, started_at: i64) -> Self {
        Self {
            operation_id,
            started_at,
        }
    }
    pub(crate) fn operation_id(&self) -> &str {
        &self.operation_id
    }
    pub(crate) const fn started_at(&self) -> i64 {
        self.started_at
    }
}
