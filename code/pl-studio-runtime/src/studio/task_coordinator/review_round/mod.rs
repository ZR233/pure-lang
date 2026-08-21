//! ReviewRound aggregate with reviewer execution encoded in its lifecycle state.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::{
    ReviewDesignReference, ReviewFileCoverage, ReviewFinding, ReviewScope, ReviewVerdict,
    ThreadExecutionStatus,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PendingReviewerState {
    Queued,
    Running,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum FailedReviewerState {
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PendingReviewState {
    pub(crate) reviewer: PendingReviewerState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CompletedReviewState {
    pub(crate) summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FailedReviewState {
    pub(crate) reviewer: FailedReviewerState,
    pub(crate) error: String,
    pub(crate) summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub(crate) enum ReviewRoundState {
    Pending(PendingReviewState),
    Pass(CompletedReviewState),
    ChangesRequired(CompletedReviewState),
    Blocked(CompletedReviewState),
    Failed(FailedReviewState),
}

impl ReviewRoundState {
    pub(crate) fn pending() -> Self {
        Self::Pending(PendingReviewState {
            reviewer: PendingReviewerState::Queued,
        })
    }

    pub(crate) fn running() -> Self {
        Self::Pending(PendingReviewState {
            reviewer: PendingReviewerState::Running,
        })
    }

    pub(crate) fn pass(summary: String) -> Self {
        Self::Pass(CompletedReviewState { summary })
    }

    pub(crate) fn changes_required(summary: String) -> Self {
        Self::ChangesRequired(CompletedReviewState { summary })
    }

    pub(crate) fn blocked(summary: String) -> Self {
        Self::Blocked(CompletedReviewState { summary })
    }

    pub(crate) fn failed(error: String, summary: String) -> Self {
        Self::Failed(FailedReviewState {
            reviewer: FailedReviewerState::Failed,
            error,
            summary,
        })
    }

    pub(crate) fn cancelled(error: String, summary: String) -> Self {
        Self::Failed(FailedReviewState {
            reviewer: FailedReviewerState::Cancelled,
            error,
            summary,
        })
    }

    pub(crate) const fn verdict(&self) -> ReviewVerdict {
        match self {
            Self::Pending(_) => ReviewVerdict::Pending,
            Self::Pass(_) => ReviewVerdict::Pass,
            Self::ChangesRequired(_) => ReviewVerdict::ChangesRequired,
            Self::Blocked(_) => ReviewVerdict::Blocked,
            Self::Failed(_) => ReviewVerdict::Failed,
        }
    }

    pub(crate) const fn reviewer_status(&self) -> ThreadExecutionStatus {
        match self {
            Self::Pending(state) => match state.reviewer {
                PendingReviewerState::Queued => ThreadExecutionStatus::Queued,
                PendingReviewerState::Running => ThreadExecutionStatus::Running,
            },
            Self::Pass(_) | Self::ChangesRequired(_) | Self::Blocked(_) => {
                ThreadExecutionStatus::Completed
            }
            Self::Failed(state) => match state.reviewer {
                FailedReviewerState::Failed => ThreadExecutionStatus::Failed,
                FailedReviewerState::Cancelled => ThreadExecutionStatus::Cancelled,
            },
        }
    }

    pub(crate) fn summary(&self) -> Option<&str> {
        match self {
            Self::Pending(_) => None,
            Self::Pass(state) | Self::ChangesRequired(state) | Self::Blocked(state) => {
                Some(&state.summary)
            }
            Self::Failed(state) => Some(&state.summary),
        }
    }

    pub(crate) fn error(&self) -> Option<&str> {
        match self {
            Self::Failed(state) => Some(&state.error),
            _ => None,
        }
    }
}

pub(crate) fn decode_review_round_state(value: &str) -> Result<ReviewRoundState> {
    serde_json::from_str(value).context("invalid stored ReviewRound state JSON")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReviewRoundRecord {
    pub(crate) id: String,
    pub(crate) task_run_id: String,
    pub(crate) round: u32,
    pub(crate) scope: ReviewScope,
    pub(crate) work_unit_id: Option<String>,
    pub(crate) completion_id: Option<String>,
    pub(crate) completion_revision: Option<u32>,
    pub(crate) reviewed_head: String,
    pub(crate) requested_by_call_id: String,
    pub(crate) reviewer_thread_id: Option<String>,
    pub(crate) state: ReviewRoundState,
    pub(crate) design_references: Vec<ReviewDesignReference>,
    pub(crate) findings: Vec<ReviewFinding>,
    #[serde(skip_serializing)]
    pub(crate) file_reviews: Option<ReviewFileCoverage>,
    pub(crate) revision: u64,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
}

impl ReviewRoundRecord {
    pub(crate) const fn verdict(&self) -> ReviewVerdict {
        self.state.verdict()
    }

    pub(crate) const fn reviewer_status(&self) -> ThreadExecutionStatus {
        self.state.reviewer_status()
    }

    pub(crate) fn reviewer_error(&self) -> Option<&str> {
        self.state.error()
    }

    pub(crate) fn summary(&self) -> Option<&str> {
        self.state.summary()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_round_trip_as_a_single_tagged_enum() {
        let states = [
            ReviewRoundState::pending(),
            ReviewRoundState::running(),
            ReviewRoundState::pass("pass".to_string()),
            ReviewRoundState::changes_required("changes".to_string()),
            ReviewRoundState::blocked("blocked".to_string()),
            ReviewRoundState::failed("failed".to_string(), "summary".to_string()),
            ReviewRoundState::cancelled("cancelled".to_string(), "summary".to_string()),
        ];

        for state in states {
            let value = serde_json::to_value(&state).unwrap();
            assert_eq!(value["kind"], state.verdict().as_str());
            let decoded: ReviewRoundState = serde_json::from_value(value).unwrap();
            assert_eq!(decoded, state);
        }
    }
}
