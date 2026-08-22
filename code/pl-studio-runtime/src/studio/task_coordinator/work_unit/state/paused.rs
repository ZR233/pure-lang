use serde::{Deserialize, Serialize};

use super::ExecutorContinuationState;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub(crate) enum WorkUnitPauseReason {
    Budget {
        limit: pl_protocol::BudgetLimitSnapshot,
    },
    Operational {
        operation_id: String,
        detail: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PausedWorkUnit {
    pub(super) reason: WorkUnitPauseReason,
    pub(super) continuation: ExecutorContinuationState,
}

impl PausedWorkUnit {
    pub(crate) const fn reason(&self) -> &WorkUnitPauseReason {
        &self.reason
    }

    pub(crate) const fn continuation(&self) -> &ExecutorContinuationState {
        &self.continuation
    }
}
