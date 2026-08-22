//! Task failure 状态与纯转换规则。

mod open;
mod resolved;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use pl_protocol::{ProviderFailureKind, TurnFailure, TurnFailureCategory};

use open::OpenTaskFailure;
use resolved::ResolvedTaskFailure;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum TaskFailureDisposition {
    Recoverable,
    Fatal,
}

impl TaskFailureDisposition {
    pub(crate) fn for_turn_failure(failure: &TurnFailure) -> Self {
        match failure.category {
            TurnFailureCategory::ProviderCapacity | TurnFailureCategory::Validation => {
                Self::Recoverable
            }
            TurnFailureCategory::Provider
                if failure.retry.is_retryable()
                    || matches!(
                        failure.provider_kind,
                        Some(ProviderFailureKind::Capacity | ProviderFailureKind::Transport)
                    ) =>
            {
                Self::Recoverable
            }
            TurnFailureCategory::Provider
            | TurnFailureCategory::Tool
            | TurnFailureCategory::Internal => Self::Fatal,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum TaskFailureStateKind {
    OpenRecoverable,
    OpenFatal,
    Resolved,
}

impl TaskFailureStateKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::OpenRecoverable => "openRecoverable",
            Self::OpenFatal => "openFatal",
            Self::Resolved => "resolved",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub(crate) enum TaskFailureState {
    OpenRecoverable(OpenTaskFailure),
    OpenFatal(OpenTaskFailure),
    Resolved(ResolvedTaskFailure),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TaskFailureCommand {
    Resolve {
        operation_id: String,
        resolved_at: i64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TaskFailureTransitionDecision {
    next_state: TaskFailureState,
    changed: bool,
}

impl TaskFailureTransitionDecision {
    pub(crate) fn next_state(self) -> TaskFailureState {
        self.next_state
    }

    pub(crate) const fn changed(&self) -> bool {
        self.changed
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(crate) enum TaskFailureTransitionError {
    #[error(
        "Task failure {task_failure_id} revision is stale: expected {expected}, actual {actual}, command {command:?}"
    )]
    StaleRevision {
        task_failure_id: String,
        expected: u64,
        actual: u64,
        command: TaskFailureCommand,
    },
    #[error("Task failure {task_failure_id} in {current:?} rejects command {command:?}")]
    IllegalTransition {
        task_failure_id: String,
        current: TaskFailureStateKind,
        command: TaskFailureCommand,
    },
}

impl TaskFailureState {
    pub(crate) fn open(failure: TurnFailure) -> Self {
        match TaskFailureDisposition::for_turn_failure(&failure) {
            TaskFailureDisposition::Recoverable => {
                Self::OpenRecoverable(OpenTaskFailure::new(failure))
            }
            TaskFailureDisposition::Fatal => Self::OpenFatal(OpenTaskFailure::new(failure)),
        }
    }

    pub(crate) const fn kind(&self) -> TaskFailureStateKind {
        match self {
            Self::OpenRecoverable(_) => TaskFailureStateKind::OpenRecoverable,
            Self::OpenFatal(_) => TaskFailureStateKind::OpenFatal,
            Self::Resolved(_) => TaskFailureStateKind::Resolved,
        }
    }

    pub(crate) const fn disposition(&self) -> TaskFailureDisposition {
        match self {
            Self::OpenRecoverable(_) | Self::Resolved(_) => TaskFailureDisposition::Recoverable,
            Self::OpenFatal(_) => TaskFailureDisposition::Fatal,
        }
    }

    pub(crate) fn failure(&self) -> &TurnFailure {
        match self {
            Self::OpenRecoverable(state) | Self::OpenFatal(state) => state.failure(),
            Self::Resolved(state) => state.failure(),
        }
    }

    pub(crate) fn decide(
        &self,
        task_failure_id: &str,
        command: TaskFailureCommand,
    ) -> Result<TaskFailureTransitionDecision, TaskFailureTransitionError> {
        let next_state = match (self, &command) {
            (
                Self::OpenRecoverable(state),
                TaskFailureCommand::Resolve {
                    operation_id,
                    resolved_at,
                },
            ) => Self::Resolved(ResolvedTaskFailure::new(
                state.failure().clone(),
                operation_id.clone(),
                *resolved_at,
            )),
            (
                Self::Resolved(state),
                TaskFailureCommand::Resolve {
                    operation_id,
                    resolved_at,
                },
            ) if state.operation_id() == operation_id && state.resolved_at() == *resolved_at => {
                self.clone()
            }
            _ => {
                return Err(TaskFailureTransitionError::IllegalTransition {
                    task_failure_id: task_failure_id.to_string(),
                    current: self.kind(),
                    command,
                });
            }
        };
        let changed = next_state != *self;
        Ok(TaskFailureTransitionDecision {
            next_state,
            changed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recoverable_failure() -> TurnFailure {
        TurnFailure::permanent(TurnFailureCategory::ProviderCapacity, "busy")
    }

    #[test]
    fn recoverable_failure_resolves_once_with_an_exact_idempotency_key() {
        let open = TaskFailureState::open(recoverable_failure());
        let command = TaskFailureCommand::Resolve {
            operation_id: "op-1".to_string(),
            resolved_at: 12,
        };
        let resolved = open
            .decide("failure-1", command.clone())
            .unwrap()
            .next_state();
        assert_eq!(resolved.kind(), TaskFailureStateKind::Resolved);
        assert!(!resolved.decide("failure-1", command).unwrap().changed());
        assert!(
            resolved
                .decide(
                    "failure-1",
                    TaskFailureCommand::Resolve {
                        operation_id: "op-2".to_string(),
                        resolved_at: 13,
                    }
                )
                .is_err()
        );
    }

    #[test]
    fn state_round_trip_rejects_missing_payloads() {
        let state = TaskFailureState::open(recoverable_failure());
        let value = serde_json::to_value(&state).unwrap();
        assert_eq!(value["kind"], "openRecoverable");
        assert_eq!(
            serde_json::from_value::<TaskFailureState>(value).unwrap(),
            state
        );
        assert!(
            serde_json::from_value::<TaskFailureState>(
                serde_json::json!({"kind":"openRecoverable"})
            )
            .is_err()
        );
    }
}
