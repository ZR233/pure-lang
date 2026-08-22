use serde::{Deserialize, Serialize};

use super::ExecutorContinuationState;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub(crate) enum RunningActivity {
    Allocated,
    Active { turn_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RunningWorkUnit {
    pub(super) activity: RunningActivity,
    pub(super) continuation: ExecutorContinuationState,
}

impl RunningWorkUnit {
    pub(super) fn allocated(continuation_revision: u64, slice_count: u32) -> Self {
        Self {
            activity: RunningActivity::Allocated,
            continuation: ExecutorContinuationState::idle(continuation_revision, slice_count),
        }
    }

    pub(crate) const fn activity(&self) -> &RunningActivity {
        &self.activity
    }

    pub(crate) const fn continuation(&self) -> &ExecutorContinuationState {
        &self.continuation
    }
}
