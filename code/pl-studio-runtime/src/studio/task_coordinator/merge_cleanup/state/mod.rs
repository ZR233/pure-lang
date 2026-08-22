mod attempting;
mod failed;
mod terminal;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use attempting::AttemptingCleanup;
use failed::FailedCleanup;
use terminal::CompletedCleanup;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum MergeCleanupStateKind {
    Pending,
    Deferred,
    Attempting,
    Discarded,
    AlreadyAbsent,
    Failed,
}

impl MergeCleanupStateKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Deferred => "deferred",
            Self::Attempting => "attempting",
            Self::Discarded => "discarded",
            Self::AlreadyAbsent => "alreadyAbsent",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PendingCleanup {}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeferredCleanup {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub(crate) enum MergeCleanupState {
    Pending(PendingCleanup),
    Deferred(DeferredCleanup),
    Attempting(AttemptingCleanup),
    Discarded(CompletedCleanup),
    AlreadyAbsent(CompletedCleanup),
    Failed(FailedCleanup),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MergeCleanupResult {
    Discarded,
    AlreadyAbsent,
    Failed { detail: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MergeCleanupCommand {
    Attempt {
        operation_id: String,
        started_at: i64,
    },
    Complete {
        operation_id: String,
        completed_at: i64,
        result: MergeCleanupResult,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MergeCleanupTransitionDecision {
    next_state: MergeCleanupState,
    changed: bool,
}

impl MergeCleanupTransitionDecision {
    pub(crate) fn next_state(self) -> MergeCleanupState {
        self.next_state
    }
    pub(crate) const fn changed(&self) -> bool {
        self.changed
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(crate) enum MergeCleanupTransitionError {
    #[error(
        "merge cleanup {merge_id} revision is stale: expected {expected}, actual {actual}, command {command:?}"
    )]
    StaleRevision {
        merge_id: String,
        expected: u64,
        actual: u64,
        command: MergeCleanupCommand,
    },
    #[error("merge cleanup {merge_id} in {current:?} rejects command {command:?}")]
    IllegalTransition {
        merge_id: String,
        current: MergeCleanupStateKind,
        command: MergeCleanupCommand,
    },
    #[error("merge cleanup {merge_id} operation mismatch: expected {expected}, actual {actual}")]
    OperationMismatch {
        merge_id: String,
        expected: String,
        actual: String,
    },
}

impl MergeCleanupState {
    pub(crate) fn pending() -> Self {
        Self::Pending(PendingCleanup {})
    }

    pub(crate) const fn kind(&self) -> MergeCleanupStateKind {
        match self {
            Self::Pending(_) => MergeCleanupStateKind::Pending,
            Self::Deferred(_) => MergeCleanupStateKind::Deferred,
            Self::Attempting(_) => MergeCleanupStateKind::Attempting,
            Self::Discarded(_) => MergeCleanupStateKind::Discarded,
            Self::AlreadyAbsent(_) => MergeCleanupStateKind::AlreadyAbsent,
            Self::Failed(_) => MergeCleanupStateKind::Failed,
        }
    }

    pub(crate) const fn is_complete(&self) -> bool {
        matches!(self, Self::Discarded(_) | Self::AlreadyAbsent(_))
    }

    pub(crate) fn operation_id(&self) -> Option<&str> {
        match self {
            Self::Attempting(state) => Some(state.operation_id()),
            Self::Discarded(state) | Self::AlreadyAbsent(state) => Some(state.operation_id()),
            Self::Failed(state) => Some(state.operation_id()),
            Self::Pending(_) | Self::Deferred(_) => None,
        }
    }

    pub(crate) fn decide(
        &self,
        merge_id: &str,
        command: MergeCleanupCommand,
    ) -> Result<MergeCleanupTransitionDecision, MergeCleanupTransitionError> {
        let next_state = match (self, &command) {
            (
                Self::Pending(_) | Self::Deferred(_) | Self::Failed(_),
                MergeCleanupCommand::Attempt {
                    operation_id,
                    started_at,
                },
            ) => Self::Attempting(AttemptingCleanup::new(operation_id.clone(), *started_at)),
            (
                Self::Attempting(state),
                MergeCleanupCommand::Complete {
                    operation_id,
                    completed_at,
                    result,
                },
            ) => {
                if state.operation_id() != operation_id {
                    return Err(MergeCleanupTransitionError::OperationMismatch {
                        merge_id: merge_id.to_string(),
                        expected: state.operation_id().to_string(),
                        actual: operation_id.clone(),
                    });
                }
                match result {
                    MergeCleanupResult::Discarded => {
                        Self::Discarded(CompletedCleanup::new(operation_id.clone(), *completed_at))
                    }
                    MergeCleanupResult::AlreadyAbsent => Self::AlreadyAbsent(
                        CompletedCleanup::new(operation_id.clone(), *completed_at),
                    ),
                    MergeCleanupResult::Failed { detail } => Self::Failed(FailedCleanup::new(
                        operation_id.clone(),
                        *completed_at,
                        detail.clone(),
                    )),
                }
            }
            _ if is_exact_replay(self, &command) => self.clone(),
            _ => {
                return Err(MergeCleanupTransitionError::IllegalTransition {
                    merge_id: merge_id.to_string(),
                    current: self.kind(),
                    command,
                });
            }
        };
        let changed = next_state != *self;
        Ok(MergeCleanupTransitionDecision {
            next_state,
            changed,
        })
    }
}

fn is_exact_replay(state: &MergeCleanupState, command: &MergeCleanupCommand) -> bool {
    match (state, command) {
        (
            MergeCleanupState::Attempting(value),
            MergeCleanupCommand::Attempt {
                operation_id,
                started_at,
            },
        ) => value.operation_id() == operation_id && value.started_at() == *started_at,
        (
            MergeCleanupState::Discarded(value),
            MergeCleanupCommand::Complete {
                operation_id,
                completed_at,
                result: MergeCleanupResult::Discarded,
            },
        )
        | (
            MergeCleanupState::AlreadyAbsent(value),
            MergeCleanupCommand::Complete {
                operation_id,
                completed_at,
                result: MergeCleanupResult::AlreadyAbsent,
            },
        ) => value.operation_id() == operation_id && value.completed_at() == *completed_at,
        (
            MergeCleanupState::Failed(value),
            MergeCleanupCommand::Complete {
                operation_id,
                completed_at,
                result: MergeCleanupResult::Failed { detail },
            },
        ) => {
            value.operation_id() == operation_id
                && value.failed_at() == *completed_at
                && value.detail() == detail
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_requires_matching_attempt_operation() {
        let attempting = MergeCleanupState::pending()
            .decide(
                "merge-1",
                MergeCleanupCommand::Attempt {
                    operation_id: "op-1".to_string(),
                    started_at: 1,
                },
            )
            .unwrap()
            .next_state();
        assert!(
            attempting
                .decide(
                    "merge-1",
                    MergeCleanupCommand::Complete {
                        operation_id: "op-2".to_string(),
                        completed_at: 2,
                        result: MergeCleanupResult::Discarded,
                    }
                )
                .is_err()
        );
        let discarded = attempting
            .decide(
                "merge-1",
                MergeCleanupCommand::Complete {
                    operation_id: "op-1".to_string(),
                    completed_at: 2,
                    result: MergeCleanupResult::Discarded,
                },
            )
            .unwrap()
            .next_state();
        assert_eq!(discarded.kind(), MergeCleanupStateKind::Discarded);
        assert!(
            discarded
                .decide(
                    "merge-1",
                    MergeCleanupCommand::Attempt {
                        operation_id: "op-3".to_string(),
                        started_at: 3
                    }
                )
                .is_err()
        );
    }
}
