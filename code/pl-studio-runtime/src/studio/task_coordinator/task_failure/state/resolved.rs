use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResolvedTaskFailure {
    failure: pl_protocol::TurnFailure,
    operation_id: String,
    resolved_at: i64,
}

impl ResolvedTaskFailure {
    pub(crate) fn new(
        failure: pl_protocol::TurnFailure,
        operation_id: String,
        resolved_at: i64,
    ) -> Self {
        Self {
            failure,
            operation_id,
            resolved_at,
        }
    }

    pub(crate) fn failure(&self) -> &pl_protocol::TurnFailure {
        &self.failure
    }

    pub(crate) fn operation_id(&self) -> &str {
        &self.operation_id
    }

    pub(crate) const fn resolved_at(&self) -> i64 {
        self.resolved_at
    }
}
