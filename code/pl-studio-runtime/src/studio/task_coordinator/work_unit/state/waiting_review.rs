use serde::{Deserialize, Serialize};

use super::ExecutorContinuationState;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub(crate) enum ExecutorTerminalOutcome {
    Completed {
        source_turn_id: String,
        detail: String,
    },
    Failed {
        source_turn_id: String,
        detail: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AwaitingReport {
    pub(super) outcome: ExecutorTerminalOutcome,
    pub(super) continuation: ExecutorContinuationState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReadyForReview {
    pub(super) completion_id: String,
    pub(super) completion_revision: u32,
    pub(super) verification_summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReviewInProgress {
    pub(super) completion_id: String,
    pub(super) completion_revision: u32,
    pub(super) review_round_id: String,
    pub(super) verification_summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub(crate) enum WaitingReviewPhase {
    AwaitingReport(AwaitingReport),
    Ready(ReadyForReview),
    Reviewing(ReviewInProgress),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WaitingReviewWorkUnit {
    pub(super) phase: WaitingReviewPhase,
}

impl WaitingReviewWorkUnit {
    pub(crate) const fn phase(&self) -> &WaitingReviewPhase {
        &self.phase
    }
}

impl AwaitingReport {
    pub(crate) const fn outcome(&self) -> &ExecutorTerminalOutcome {
        &self.outcome
    }

    pub(crate) const fn continuation(&self) -> &ExecutorContinuationState {
        &self.continuation
    }
}

impl ReadyForReview {
    pub(crate) fn completion_id(&self) -> &str {
        &self.completion_id
    }

    pub(crate) const fn completion_revision(&self) -> u32 {
        self.completion_revision
    }

    pub(crate) fn verification_summary(&self) -> &str {
        &self.verification_summary
    }
}

impl ReviewInProgress {
    pub(crate) fn completion_id(&self) -> &str {
        &self.completion_id
    }

    pub(crate) const fn completion_revision(&self) -> u32 {
        self.completion_revision
    }

    pub(crate) fn review_round_id(&self) -> &str {
        &self.review_round_id
    }

    pub(crate) fn verification_summary(&self) -> &str {
        &self.verification_summary
    }
}
