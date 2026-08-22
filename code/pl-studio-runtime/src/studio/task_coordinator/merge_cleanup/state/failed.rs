use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FailedCleanup {
    operation_id: String,
    failed_at: i64,
    detail: String,
}

impl FailedCleanup {
    pub(crate) fn new(operation_id: String, failed_at: i64, detail: String) -> Self {
        Self {
            operation_id,
            failed_at,
            detail,
        }
    }
    pub(crate) fn operation_id(&self) -> &str {
        &self.operation_id
    }
    pub(crate) const fn failed_at(&self) -> i64 {
        self.failed_at
    }
    pub(crate) fn detail(&self) -> &str {
        &self.detail
    }
}
