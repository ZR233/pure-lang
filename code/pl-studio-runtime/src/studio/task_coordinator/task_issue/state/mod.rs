//! Task issue 状态与纯转换规则。

mod open;
mod resolved;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use pl_protocol::{ProviderFailureKind, TurnFailure, TurnFailureCategory};

use open::OpenTaskIssue;
use resolved::ResolvedTaskIssue;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum TaskIssueDisposition {
    Recoverable,
    Fatal,
}

impl TaskIssueDisposition {
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
pub(crate) enum TaskIssueStateKind {
    OpenRecoverable,
    OpenFatal,
    Resolved,
}

impl TaskIssueStateKind {
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
pub(crate) enum TaskIssueState {
    OpenRecoverable(OpenTaskIssue),
    OpenFatal(OpenTaskIssue),
    Resolved(ResolvedTaskIssue),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TaskIssueCommand {
    Resolve {
        operation_id: String,
        summary: String,
        evidence: String,
        resolved_at: i64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TaskIssueTransitionDecision {
    next_state: TaskIssueState,
    changed: bool,
}

impl TaskIssueTransitionDecision {
    pub(crate) fn next_state(self) -> TaskIssueState {
        self.next_state
    }

    pub(crate) const fn changed(&self) -> bool {
        self.changed
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(crate) enum TaskIssueTransitionError {
    #[error(
        "Task issue {task_issue_id} revision is stale: expected {expected}, actual {actual}, command {command:?}"
    )]
    StaleRevision {
        task_issue_id: String,
        expected: u64,
        actual: u64,
        command: TaskIssueCommand,
    },
    #[error("Task issue {task_issue_id} in {current:?} rejects command {command:?}")]
    IllegalTransition {
        task_issue_id: String,
        current: TaskIssueStateKind,
        command: TaskIssueCommand,
    },
}

impl TaskIssueState {
    pub(crate) fn open(failure: TurnFailure) -> Self {
        match TaskIssueDisposition::for_turn_failure(&failure) {
            TaskIssueDisposition::Recoverable => Self::OpenRecoverable(OpenTaskIssue::new(failure)),
            TaskIssueDisposition::Fatal => Self::OpenFatal(OpenTaskIssue::new(failure)),
        }
    }

    pub(crate) const fn kind(&self) -> TaskIssueStateKind {
        match self {
            Self::OpenRecoverable(_) => TaskIssueStateKind::OpenRecoverable,
            Self::OpenFatal(_) => TaskIssueStateKind::OpenFatal,
            Self::Resolved(_) => TaskIssueStateKind::Resolved,
        }
    }

    pub(crate) const fn disposition(&self) -> TaskIssueDisposition {
        match self {
            Self::OpenRecoverable(_) | Self::Resolved(_) => TaskIssueDisposition::Recoverable,
            Self::OpenFatal(_) => TaskIssueDisposition::Fatal,
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
        task_issue_id: &str,
        command: TaskIssueCommand,
    ) -> Result<TaskIssueTransitionDecision, TaskIssueTransitionError> {
        let next_state = match (self, &command) {
            (
                Self::OpenRecoverable(state),
                TaskIssueCommand::Resolve {
                    operation_id,
                    summary,
                    evidence,
                    resolved_at,
                },
            ) => Self::Resolved(ResolvedTaskIssue::new(
                state.failure().clone(),
                operation_id.clone(),
                summary.clone(),
                evidence.clone(),
                *resolved_at,
            )),
            (
                Self::Resolved(state),
                TaskIssueCommand::Resolve {
                    operation_id,
                    summary,
                    evidence,
                    resolved_at,
                },
            ) if state.operation_id() == operation_id
                && state.summary() == summary
                && state.evidence() == evidence
                && state.resolved_at() == *resolved_at =>
            {
                self.clone()
            }
            _ => {
                return Err(TaskIssueTransitionError::IllegalTransition {
                    task_issue_id: task_issue_id.to_string(),
                    current: self.kind(),
                    command,
                });
            }
        };
        let changed = next_state != *self;
        Ok(TaskIssueTransitionDecision {
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
        let open = TaskIssueState::open(recoverable_failure());
        let command = TaskIssueCommand::Resolve {
            operation_id: "op-1".to_string(),
            summary: "fixed".to_string(),
            evidence: "verified".to_string(),
            resolved_at: 12,
        };
        let resolved = open
            .decide("failure-1", command.clone())
            .unwrap()
            .next_state();
        assert_eq!(resolved.kind(), TaskIssueStateKind::Resolved);
        assert!(!resolved.decide("failure-1", command).unwrap().changed());
        assert!(
            resolved
                .decide(
                    "failure-1",
                    TaskIssueCommand::Resolve {
                        operation_id: "op-2".to_string(),
                        summary: "fixed differently".to_string(),
                        evidence: "verified differently".to_string(),
                        resolved_at: 13,
                    }
                )
                .is_err()
        );
    }

    #[test]
    fn state_round_trip_rejects_missing_payloads() {
        let state = TaskIssueState::open(recoverable_failure());
        let value = serde_json::to_value(&state).unwrap();
        assert_eq!(value["kind"], "openRecoverable");
        assert_eq!(
            serde_json::from_value::<TaskIssueState>(value).unwrap(),
            state
        );
        assert!(
            serde_json::from_value::<TaskIssueState>(serde_json::json!({"kind":"openRecoverable"}))
                .is_err()
        );
    }
}
