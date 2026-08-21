//! ReviewRound aggregate with reviewer execution encoded in its lifecycle state.

use anyhow::{Context, Result, bail};
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

    pub(crate) fn from_parts(
        verdict: ReviewVerdict,
        reviewer: ThreadExecutionStatus,
        summary: Option<String>,
        error: Option<String>,
    ) -> Result<Self> {
        let completed = || -> Result<CompletedReviewState> {
            Ok(CompletedReviewState {
                summary: summary
                    .clone()
                    .context("completed review requires a summary")?,
            })
        };
        match (verdict, reviewer) {
            (ReviewVerdict::Pending, ThreadExecutionStatus::Queued) => {
                Ok(Self::Pending(PendingReviewState {
                    reviewer: PendingReviewerState::Queued,
                }))
            }
            (ReviewVerdict::Pending, ThreadExecutionStatus::Running) => {
                Ok(Self::Pending(PendingReviewState {
                    reviewer: PendingReviewerState::Running,
                }))
            }
            (ReviewVerdict::Pass, ThreadExecutionStatus::Completed) => Ok(Self::Pass(completed()?)),
            (ReviewVerdict::ChangesRequired, ThreadExecutionStatus::Completed) => {
                Ok(Self::ChangesRequired(completed()?))
            }
            (ReviewVerdict::Blocked, ThreadExecutionStatus::Completed) => {
                Ok(Self::Blocked(completed()?))
            }
            (ReviewVerdict::Failed, ThreadExecutionStatus::Failed) => {
                Ok(Self::Failed(FailedReviewState {
                    reviewer: FailedReviewerState::Failed,
                    error: error.context("failed review requires an error")?,
                    summary: summary.context("failed review requires a summary")?,
                }))
            }
            (ReviewVerdict::Failed, ThreadExecutionStatus::Cancelled) => {
                Ok(Self::Failed(FailedReviewState {
                    reviewer: FailedReviewerState::Cancelled,
                    error: error.context("cancelled review requires an error")?,
                    summary: summary.context("cancelled review requires a summary")?,
                }))
            }
            (verdict, reviewer) => bail!(
                "invalid ReviewRound state combination: {} + {}",
                verdict.as_str(),
                reviewer.as_str()
            ),
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
    fn legal_review_states_round_trip_and_reject_split_status_pairs() {
        let cases = [
            (ReviewVerdict::Pending, ThreadExecutionStatus::Queued),
            (ReviewVerdict::Pending, ThreadExecutionStatus::Running),
            (ReviewVerdict::Pass, ThreadExecutionStatus::Completed),
            (
                ReviewVerdict::ChangesRequired,
                ThreadExecutionStatus::Completed,
            ),
            (ReviewVerdict::Blocked, ThreadExecutionStatus::Completed),
            (ReviewVerdict::Failed, ThreadExecutionStatus::Failed),
            (ReviewVerdict::Failed, ThreadExecutionStatus::Cancelled),
        ];

        for (verdict, reviewer) in cases {
            let completed = reviewer == ThreadExecutionStatus::Completed;
            let failed = verdict == ReviewVerdict::Failed;
            let state = ReviewRoundState::from_parts(
                verdict,
                reviewer,
                (completed || failed).then(|| "review summary".to_string()),
                failed.then(|| "review error".to_string()),
            )
            .unwrap();
            let value = serde_json::to_value(&state).unwrap();
            assert_eq!(value["kind"], verdict.as_str());
            let decoded: ReviewRoundState = serde_json::from_value(value).unwrap();
            assert_eq!(decoded, state);
        }

        assert!(
            ReviewRoundState::from_parts(
                ReviewVerdict::Pass,
                ThreadExecutionStatus::Running,
                Some("summary".to_string()),
                None,
            )
            .is_err()
        );
        assert!(
            ReviewRoundState::from_parts(
                ReviewVerdict::Pass,
                ThreadExecutionStatus::Completed,
                None,
                None,
            )
            .is_err()
        );
    }
}
