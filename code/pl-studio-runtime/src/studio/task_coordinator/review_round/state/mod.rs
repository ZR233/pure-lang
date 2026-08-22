//! ReviewRound 生命周期状态、命令与纯转换判定。

mod blocked;
mod cancelled;
mod changes_required;
mod dispatched;
mod failed;
mod passed;
mod pending_dispatch;
mod running;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::super::ReviewVerdict;

pub(crate) use blocked::BlockedReview;
pub(crate) use cancelled::CancelledReview;
pub(crate) use changes_required::ChangesRequiredReview;
pub(crate) use dispatched::DispatchedReview;
pub(crate) use failed::FailedReview;
pub(crate) use passed::PassedReview;
pub(crate) use pending_dispatch::PendingReviewDispatch;
pub(crate) use running::RunningReview;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ReviewRoundStateKind {
    PendingDispatch,
    Dispatched,
    Running,
    Passed,
    ChangesRequired,
    Blocked,
    Failed,
    Cancelled,
}

impl ReviewRoundStateKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::PendingDispatch => "pendingDispatch",
            Self::Dispatched => "dispatched",
            Self::Running => "running",
            Self::Passed => "passed",
            Self::ChangesRequired => "changesRequired",
            Self::Blocked => "blocked",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub(crate) const fn is_active(self) -> bool {
        matches!(
            self,
            Self::PendingDispatch | Self::Dispatched | Self::Running
        )
    }

    pub(crate) const fn is_terminal(self) -> bool {
        !self.is_active()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub(crate) enum ReviewRoundState {
    PendingDispatch(PendingReviewDispatch),
    Dispatched(DispatchedReview),
    Running(RunningReview),
    Passed(PassedReview),
    ChangesRequired(ChangesRequiredReview),
    Blocked(BlockedReview),
    Failed(FailedReview),
    Cancelled(CancelledReview),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReviewRoundCommand {
    Dispatch {
        reviewer_thread_id: String,
    },
    Start {
        reviewer_thread_id: String,
    },
    Pass {
        reviewer_thread_id: String,
        summary: String,
    },
    RequireChanges {
        reviewer_thread_id: String,
        summary: String,
    },
    Block {
        reviewer_thread_id: String,
        summary: String,
    },
    Fail {
        reviewer_thread_id: Option<String>,
        error: String,
        summary: String,
    },
    Cancel {
        reviewer_thread_id: Option<String>,
        reason: String,
        summary: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReviewRoundTransitionDecision {
    next_state: ReviewRoundState,
    changed: bool,
}

impl ReviewRoundTransitionDecision {
    pub(crate) fn next_state(self) -> ReviewRoundState {
        self.next_state
    }

    pub(crate) const fn changed(&self) -> bool {
        self.changed
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(crate) enum ReviewRoundTransitionError {
    #[error(
        "ReviewRound {review_round_id} revision is stale: expected {expected}, actual {actual}, command {command:?}"
    )]
    StaleRevision {
        review_round_id: String,
        expected: u64,
        actual: u64,
        command: Box<ReviewRoundCommand>,
    },
    #[error("ReviewRound {review_round_id} in {current:?} rejects command {command:?}")]
    IllegalTransition {
        review_round_id: String,
        current: ReviewRoundStateKind,
        command: Box<ReviewRoundCommand>,
    },
    #[error(
        "ReviewRound {review_round_id} belongs to reviewer {expected}, not {actual}, for command {command:?}"
    )]
    ReviewerMismatch {
        review_round_id: String,
        expected: String,
        actual: String,
        command: Box<ReviewRoundCommand>,
    },
}

impl ReviewRoundState {
    pub(crate) fn pending_dispatch() -> Self {
        Self::PendingDispatch(PendingReviewDispatch::new())
    }

    pub(crate) const fn kind(&self) -> ReviewRoundStateKind {
        match self {
            Self::PendingDispatch(_) => ReviewRoundStateKind::PendingDispatch,
            Self::Dispatched(_) => ReviewRoundStateKind::Dispatched,
            Self::Running(_) => ReviewRoundStateKind::Running,
            Self::Passed(_) => ReviewRoundStateKind::Passed,
            Self::ChangesRequired(_) => ReviewRoundStateKind::ChangesRequired,
            Self::Blocked(_) => ReviewRoundStateKind::Blocked,
            Self::Failed(_) => ReviewRoundStateKind::Failed,
            Self::Cancelled(_) => ReviewRoundStateKind::Cancelled,
        }
    }

    pub(crate) const fn verdict(&self) -> ReviewVerdict {
        match self {
            Self::PendingDispatch(_) | Self::Dispatched(_) | Self::Running(_) => {
                ReviewVerdict::Pending
            }
            Self::Passed(_) => ReviewVerdict::Pass,
            Self::ChangesRequired(_) => ReviewVerdict::ChangesRequired,
            Self::Blocked(_) => ReviewVerdict::Blocked,
            Self::Failed(_) | Self::Cancelled(_) => ReviewVerdict::Failed,
        }
    }

    pub(crate) fn reviewer_thread_id(&self) -> Option<&str> {
        match self {
            Self::PendingDispatch(_) => None,
            Self::Dispatched(state) => Some(state.reviewer_thread_id()),
            Self::Running(state) => Some(state.reviewer_thread_id()),
            Self::Passed(state) => Some(state.reviewer_thread_id()),
            Self::ChangesRequired(state) => Some(state.reviewer_thread_id()),
            Self::Blocked(state) => Some(state.reviewer_thread_id()),
            Self::Failed(state) => state.reviewer_thread_id(),
            Self::Cancelled(state) => state.reviewer_thread_id(),
        }
    }

    pub(crate) fn summary(&self) -> Option<&str> {
        match self {
            Self::PendingDispatch(_) | Self::Dispatched(_) | Self::Running(_) => None,
            Self::Passed(state) => Some(state.summary()),
            Self::ChangesRequired(state) => Some(state.summary()),
            Self::Blocked(state) => Some(state.summary()),
            Self::Failed(state) => Some(state.summary()),
            Self::Cancelled(state) => Some(state.summary()),
        }
    }

    pub(crate) fn error(&self) -> Option<&str> {
        match self {
            Self::Failed(state) => Some(state.error()),
            Self::Cancelled(state) => Some(state.reason()),
            Self::PendingDispatch(_)
            | Self::Dispatched(_)
            | Self::Running(_)
            | Self::Passed(_)
            | Self::ChangesRequired(_)
            | Self::Blocked(_) => None,
        }
    }

    pub(crate) fn decide(
        &self,
        review_round_id: &str,
        command: ReviewRoundCommand,
    ) -> Result<ReviewRoundTransitionDecision, ReviewRoundTransitionError> {
        let next_state = match (self, &command) {
            (Self::PendingDispatch(_), ReviewRoundCommand::Dispatch { reviewer_thread_id }) => {
                Self::Dispatched(DispatchedReview::new(reviewer_thread_id.clone()))
            }
            (Self::Dispatched(state), ReviewRoundCommand::Start { reviewer_thread_id }) => {
                ensure_reviewer(
                    review_round_id,
                    state.reviewer_thread_id(),
                    reviewer_thread_id,
                    &command,
                )?;
                Self::Running(RunningReview::new(reviewer_thread_id.clone()))
            }
            (
                Self::Running(state),
                ReviewRoundCommand::Pass {
                    reviewer_thread_id,
                    summary,
                },
            ) => {
                ensure_reviewer(
                    review_round_id,
                    state.reviewer_thread_id(),
                    reviewer_thread_id,
                    &command,
                )?;
                Self::Passed(PassedReview::new(
                    reviewer_thread_id.clone(),
                    summary.clone(),
                ))
            }
            (
                Self::Running(state),
                ReviewRoundCommand::RequireChanges {
                    reviewer_thread_id,
                    summary,
                },
            ) => {
                ensure_reviewer(
                    review_round_id,
                    state.reviewer_thread_id(),
                    reviewer_thread_id,
                    &command,
                )?;
                Self::ChangesRequired(ChangesRequiredReview::new(
                    reviewer_thread_id.clone(),
                    summary.clone(),
                ))
            }
            (
                Self::Running(state),
                ReviewRoundCommand::Block {
                    reviewer_thread_id,
                    summary,
                },
            ) => {
                ensure_reviewer(
                    review_round_id,
                    state.reviewer_thread_id(),
                    reviewer_thread_id,
                    &command,
                )?;
                Self::Blocked(BlockedReview::new(
                    reviewer_thread_id.clone(),
                    summary.clone(),
                ))
            }
            (
                Self::PendingDispatch(_) | Self::Dispatched(_),
                ReviewRoundCommand::Fail {
                    reviewer_thread_id,
                    error,
                    summary,
                },
            )
            | (
                Self::Running(_),
                ReviewRoundCommand::Fail {
                    reviewer_thread_id,
                    error,
                    summary,
                },
            ) => {
                ensure_optional_reviewer(
                    review_round_id,
                    self.reviewer_thread_id(),
                    reviewer_thread_id.as_deref(),
                    &command,
                )?;
                Self::Failed(FailedReview::new(
                    reviewer_thread_id.clone(),
                    error.clone(),
                    summary.clone(),
                ))
            }
            (
                Self::PendingDispatch(_) | Self::Dispatched(_) | Self::Running(_),
                ReviewRoundCommand::Cancel {
                    reviewer_thread_id,
                    reason,
                    summary,
                },
            ) => {
                ensure_optional_reviewer(
                    review_round_id,
                    self.reviewer_thread_id(),
                    reviewer_thread_id.as_deref(),
                    &command,
                )?;
                Self::Cancelled(CancelledReview::new(
                    reviewer_thread_id.clone(),
                    reason.clone(),
                    summary.clone(),
                ))
            }
            _ if is_exact_replay(self, &command) => self.clone(),
            _ => {
                return Err(ReviewRoundTransitionError::IllegalTransition {
                    review_round_id: review_round_id.to_string(),
                    current: self.kind(),
                    command: Box::new(command),
                });
            }
        };
        let changed = next_state != *self;
        Ok(ReviewRoundTransitionDecision {
            next_state,
            changed,
        })
    }
}

fn ensure_reviewer(
    review_round_id: &str,
    expected: &str,
    actual: &str,
    command: &ReviewRoundCommand,
) -> Result<(), ReviewRoundTransitionError> {
    if expected == actual {
        return Ok(());
    }
    Err(ReviewRoundTransitionError::ReviewerMismatch {
        review_round_id: review_round_id.to_string(),
        expected: expected.to_string(),
        actual: actual.to_string(),
        command: Box::new(command.clone()),
    })
}

fn ensure_optional_reviewer(
    review_round_id: &str,
    expected: Option<&str>,
    actual: Option<&str>,
    command: &ReviewRoundCommand,
) -> Result<(), ReviewRoundTransitionError> {
    match (expected, actual) {
        (Some(expected), Some(actual)) => {
            ensure_reviewer(review_round_id, expected, actual, command)
        }
        (Some(expected), None) => Err(ReviewRoundTransitionError::ReviewerMismatch {
            review_round_id: review_round_id.to_string(),
            expected: expected.to_string(),
            actual: "<none>".to_string(),
            command: Box::new(command.clone()),
        }),
        (None, Some(actual)) => Err(ReviewRoundTransitionError::ReviewerMismatch {
            review_round_id: review_round_id.to_string(),
            expected: "<none>".to_string(),
            actual: actual.to_string(),
            command: Box::new(command.clone()),
        }),
        (None, None) => Ok(()),
    }
}

fn is_exact_replay(state: &ReviewRoundState, command: &ReviewRoundCommand) -> bool {
    match (state, command) {
        (
            ReviewRoundState::Dispatched(value),
            ReviewRoundCommand::Dispatch { reviewer_thread_id },
        ) => value.reviewer_thread_id() == reviewer_thread_id,
        (ReviewRoundState::Running(value), ReviewRoundCommand::Start { reviewer_thread_id }) => {
            value.reviewer_thread_id() == reviewer_thread_id
        }
        (
            ReviewRoundState::Passed(value),
            ReviewRoundCommand::Pass {
                reviewer_thread_id,
                summary,
            },
        ) => value.reviewer_thread_id() == reviewer_thread_id && value.summary() == summary,
        (
            ReviewRoundState::ChangesRequired(value),
            ReviewRoundCommand::RequireChanges {
                reviewer_thread_id,
                summary,
            },
        ) => value.reviewer_thread_id() == reviewer_thread_id && value.summary() == summary,
        (
            ReviewRoundState::Blocked(value),
            ReviewRoundCommand::Block {
                reviewer_thread_id,
                summary,
            },
        ) => value.reviewer_thread_id() == reviewer_thread_id && value.summary() == summary,
        (
            ReviewRoundState::Failed(value),
            ReviewRoundCommand::Fail {
                reviewer_thread_id,
                error,
                summary,
            },
        ) => {
            value.reviewer_thread_id() == reviewer_thread_id.as_deref()
                && value.error() == error
                && value.summary() == summary
        }
        (
            ReviewRoundState::Cancelled(value),
            ReviewRoundCommand::Cancel {
                reviewer_thread_id,
                reason,
                summary,
            },
        ) => {
            value.reviewer_thread_id() == reviewer_thread_id.as_deref()
                && value.reason() == reason
                && value.summary() == summary
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transition_matrix_rejects_crossing_and_terminal_mutation() {
        let pending = ReviewRoundState::pending_dispatch();
        let dispatched = pending
            .decide(
                "review-1",
                ReviewRoundCommand::Dispatch {
                    reviewer_thread_id: "agent-1".to_string(),
                },
            )
            .unwrap()
            .next_state();
        let replay = dispatched
            .decide(
                "review-1",
                ReviewRoundCommand::Dispatch {
                    reviewer_thread_id: "agent-1".to_string(),
                },
            )
            .unwrap();
        assert!(!replay.changed());
        let running = dispatched
            .decide(
                "review-1",
                ReviewRoundCommand::Start {
                    reviewer_thread_id: "agent-1".to_string(),
                },
            )
            .unwrap()
            .next_state();
        let passed = running
            .decide(
                "review-1",
                ReviewRoundCommand::Pass {
                    reviewer_thread_id: "agent-1".to_string(),
                    summary: "ok".to_string(),
                },
            )
            .unwrap()
            .next_state();
        assert_eq!(passed.kind(), ReviewRoundStateKind::Passed);
        assert!(
            passed
                .decide(
                    "review-1",
                    ReviewRoundCommand::Block {
                        reviewer_thread_id: "agent-1".to_string(),
                        summary: "late".to_string()
                    }
                )
                .is_err()
        );
    }

    #[test]
    fn states_round_trip_as_adjacent_tagged_unions() {
        let states = [
            ReviewRoundState::pending_dispatch(),
            ReviewRoundState::Dispatched(DispatchedReview::new("agent".to_string())),
            ReviewRoundState::Running(RunningReview::new("agent".to_string())),
            ReviewRoundState::Passed(PassedReview::new("agent".to_string(), "pass".to_string())),
            ReviewRoundState::ChangesRequired(ChangesRequiredReview::new(
                "agent".to_string(),
                "changes".to_string(),
            )),
            ReviewRoundState::Blocked(BlockedReview::new(
                "agent".to_string(),
                "blocked".to_string(),
            )),
            ReviewRoundState::Failed(FailedReview::new(
                Some("agent".to_string()),
                "failed".to_string(),
                "summary".to_string(),
            )),
            ReviewRoundState::Cancelled(CancelledReview::new(
                Some("agent".to_string()),
                "cancelled".to_string(),
                "summary".to_string(),
            )),
        ];
        for state in states {
            let value = serde_json::to_value(&state).unwrap();
            assert_eq!(value["kind"], state.kind().as_str());
            let decoded: ReviewRoundState = serde_json::from_value(value).unwrap();
            assert_eq!(decoded, state);
        }
    }
}
