use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum WorkCompletionStatus {
    ReadyForReview,
    ChangesRequired,
    Approved,
}

impl WorkCompletionStatus {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::ReadyForReview => "readyForReview",
            Self::ChangesRequired => "changesRequired",
            Self::Approved => "approved",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReadyForReviewCompletion {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReviewedCompletion {
    review_round_id: String,
    decided_at: i64,
}

impl ReviewedCompletion {
    fn from_command(review_round_id: String, decided_at: i64) -> Self {
        Self {
            review_round_id,
            decided_at,
        }
    }

    fn review_round_id(&self) -> &str {
        &self.review_round_id
    }

    const fn decided_at(&self) -> i64 {
        self.decided_at
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub(crate) enum WorkCompletionState {
    ReadyForReview(ReadyForReviewCompletion),
    ChangesRequired(ReviewedCompletion),
    Approved(ReviewedCompletion),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WorkCompletionCommand {
    RequireChanges {
        review_round_id: String,
        decided_at: i64,
    },
    Approve {
        review_round_id: String,
        decided_at: i64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkCompletionTransitionDecision {
    next_state: WorkCompletionState,
    changed: bool,
}

impl WorkCompletionTransitionDecision {
    pub(crate) fn next_state(self) -> WorkCompletionState {
        self.next_state
    }

    pub(crate) const fn changed(&self) -> bool {
        self.changed
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(crate) enum WorkCompletionTransitionError {
    #[error(
        "work completion {completion_id} revision is stale: expected {expected}, actual {actual}, command {command:?}"
    )]
    StaleRevision {
        completion_id: String,
        expected: u64,
        actual: u64,
        command: WorkCompletionCommand,
    },
    #[error("work completion {completion_id} in {current:?} rejects command {command:?}")]
    IllegalTransition {
        completion_id: String,
        current: WorkCompletionStatus,
        command: WorkCompletionCommand,
    },
}

impl WorkCompletionState {
    pub(crate) fn ready_for_review() -> Self {
        Self::ReadyForReview(ReadyForReviewCompletion {})
    }

    pub(crate) const fn status(&self) -> WorkCompletionStatus {
        match self {
            Self::ReadyForReview(_) => WorkCompletionStatus::ReadyForReview,
            Self::ChangesRequired(_) => WorkCompletionStatus::ChangesRequired,
            Self::Approved(_) => WorkCompletionStatus::Approved,
        }
    }

    pub(crate) fn review_round_id(&self) -> Option<&str> {
        match self {
            Self::ReadyForReview(_) => None,
            Self::ChangesRequired(value) | Self::Approved(value) => Some(value.review_round_id()),
        }
    }

    pub(crate) const fn decided_at(&self) -> Option<i64> {
        match self {
            Self::ReadyForReview(_) => None,
            Self::ChangesRequired(value) | Self::Approved(value) => Some(value.decided_at()),
        }
    }

    pub(crate) fn decide(
        &self,
        completion_id: &str,
        command: WorkCompletionCommand,
    ) -> Result<WorkCompletionTransitionDecision, WorkCompletionTransitionError> {
        let next_state = match (self, &command) {
            (
                Self::ReadyForReview(_),
                WorkCompletionCommand::RequireChanges {
                    review_round_id,
                    decided_at,
                },
            ) => Self::ChangesRequired(ReviewedCompletion::from_command(
                review_round_id.clone(),
                *decided_at,
            )),
            (
                Self::ReadyForReview(_),
                WorkCompletionCommand::Approve {
                    review_round_id,
                    decided_at,
                },
            ) => Self::Approved(ReviewedCompletion::from_command(
                review_round_id.clone(),
                *decided_at,
            )),
            (
                Self::ChangesRequired(value),
                WorkCompletionCommand::RequireChanges {
                    review_round_id,
                    decided_at,
                },
            ) if value.review_round_id() == review_round_id
                && value.decided_at() == *decided_at =>
            {
                self.clone()
            }
            (
                Self::Approved(value),
                WorkCompletionCommand::Approve {
                    review_round_id,
                    decided_at,
                },
            ) if value.review_round_id() == review_round_id
                && value.decided_at() == *decided_at =>
            {
                self.clone()
            }
            _ => {
                return Err(WorkCompletionTransitionError::IllegalTransition {
                    completion_id: completion_id.to_string(),
                    current: self.status(),
                    command,
                });
            }
        };
        let changed = next_state != *self;
        Ok(WorkCompletionTransitionDecision {
            next_state,
            changed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_decision_is_terminal_and_exactly_idempotent() {
        let command = WorkCompletionCommand::Approve {
            review_round_id: "review-1".to_string(),
            decided_at: 2,
        };
        let approved = WorkCompletionState::ready_for_review()
            .decide("completion-1", command.clone())
            .unwrap()
            .next_state();
        assert!(!approved.decide("completion-1", command).unwrap().changed());
        assert!(
            approved
                .decide(
                    "completion-1",
                    WorkCompletionCommand::RequireChanges {
                        review_round_id: "review-2".to_string(),
                        decided_at: 3,
                    },
                )
                .is_err()
        );
    }
}
