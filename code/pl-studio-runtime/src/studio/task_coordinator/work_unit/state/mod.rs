mod cancelled;
mod changes_required;
mod completed;
mod continuation;
mod failed;
mod paused;
mod pending;
mod review_passed;
mod running;
mod waiting_review;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub(crate) use cancelled::CancelledWorkUnit;
pub(crate) use changes_required::ChangesRequiredWorkUnit;
pub(crate) use completed::{CompletedWorkUnit, WorkUnitCompletionOutcome};
pub(crate) use continuation::{ExecutorContinuationState, ExecutorContinuationStateKind};
pub(crate) use failed::{FailedWorkUnit, WorkUnitFailure};
pub(crate) use paused::{PausedWorkUnit, WorkUnitPauseReason};
pub(crate) use pending::PendingWorkUnit;
pub(crate) use review_passed::{ReviewPassedOutcome, ReviewPassedWorkUnit};
pub(crate) use running::{RunningActivity, RunningWorkUnit};
pub(crate) use waiting_review::{
    AwaitingReport, ExecutorTerminalOutcome, ReadyForReview, ReviewInProgress, WaitingReviewPhase,
    WaitingReviewWorkUnit,
};

use crate::studio::task_coordinator::{TaskSpawnFailure, TaskWorktreeDisposition};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum WorkUnitStateKind {
    Pending,
    Running,
    WaitingReview,
    ReviewPassed,
    ChangesRequired,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

impl WorkUnitStateKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::WaitingReview => "waitingReview",
            Self::ReviewPassed => "reviewPassed",
            Self::ChangesRequired => "changesRequired",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub(crate) const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub(crate) enum WorkUnitState {
    Pending(PendingWorkUnit),
    Running(RunningWorkUnit),
    WaitingReview(WaitingReviewWorkUnit),
    ReviewPassed(ReviewPassedWorkUnit),
    ChangesRequired(ChangesRequiredWorkUnit),
    Paused(PausedWorkUnit),
    Completed(CompletedWorkUnit),
    Failed(FailedWorkUnit),
    Cancelled(CancelledWorkUnit),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WorkUnitCommand {
    Activate,
    StartTurn {
        turn_id: String,
        reset_budget: bool,
    },
    FinishTurn {
        outcome: ExecutorTerminalOutcome,
    },
    ContinueAfterBudget {
        source_turn_id: String,
        next_slice: u32,
        limit: pl_protocol::BudgetLimitSnapshot,
    },
    PauseForBudget {
        source_turn_id: String,
        limit: pl_protocol::BudgetLimitSnapshot,
        detail: String,
    },
    PauseOperational {
        operation_id: String,
        detail: String,
    },
    SubmitCompletion {
        completion_id: String,
        completion_revision: u32,
        verification_summary: String,
    },
    BeginReview {
        review_round_id: String,
    },
    RequireChanges {
        review_round_id: String,
    },
    ReviewFailed {
        review_round_id: String,
    },
    PassReview {
        review_round_id: String,
        outcome: ReviewPassedOutcome,
    },
    CompleteMerge {
        merge_record_id: String,
    },
    FailSpawn {
        failure: Box<TaskSpawnFailure>,
    },
    FailExecution {
        operation_id: String,
        detail: String,
        disposition: TaskWorktreeDisposition,
    },
    Cancel {
        operation_id: String,
        reason: String,
        disposition: TaskWorktreeDisposition,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkUnitTransitionDecision {
    next_state: WorkUnitState,
    changed: bool,
}

impl WorkUnitTransitionDecision {
    pub(crate) fn next_state(self) -> WorkUnitState {
        self.next_state
    }

    pub(crate) const fn changed(&self) -> bool {
        self.changed
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(crate) enum WorkUnitTransitionError {
    #[error(
        "WorkUnit {work_unit_id} revision is stale: expected {expected}, actual {actual}, command {command:?}"
    )]
    StaleRevision {
        work_unit_id: String,
        expected: u64,
        actual: u64,
        command: Box<WorkUnitCommand>,
    },
    #[error("WorkUnit {work_unit_id} in {current:?} rejects command {command:?}")]
    IllegalTransition {
        work_unit_id: String,
        current: WorkUnitStateKind,
        command: Box<WorkUnitCommand>,
    },
    #[error(
        "WorkUnit {work_unit_id} command {command:?} does not match current business identity: {detail}"
    )]
    CorrelationMismatch {
        work_unit_id: String,
        command: Box<WorkUnitCommand>,
        detail: String,
    },
}

impl WorkUnitState {
    pub(crate) fn pending() -> Self {
        Self::Pending(PendingWorkUnit {})
    }

    #[cfg(test)]
    pub(crate) fn failed_for_test(
        operation_id: impl Into<String>,
        detail: impl Into<String>,
        worktree_disposition: TaskWorktreeDisposition,
    ) -> Self {
        Self::Failed(FailedWorkUnit {
            failure: WorkUnitFailure::Execution {
                operation_id: operation_id.into(),
                detail: detail.into(),
            },
            worktree_disposition,
        })
    }

    pub(crate) const fn kind(&self) -> WorkUnitStateKind {
        match self {
            Self::Pending(_) => WorkUnitStateKind::Pending,
            Self::Running(_) => WorkUnitStateKind::Running,
            Self::WaitingReview(_) => WorkUnitStateKind::WaitingReview,
            Self::ReviewPassed(_) => WorkUnitStateKind::ReviewPassed,
            Self::ChangesRequired(_) => WorkUnitStateKind::ChangesRequired,
            Self::Paused(_) => WorkUnitStateKind::Paused,
            Self::Completed(_) => WorkUnitStateKind::Completed,
            Self::Failed(_) => WorkUnitStateKind::Failed,
            Self::Cancelled(_) => WorkUnitStateKind::Cancelled,
        }
    }

    pub(crate) fn decide(
        &self,
        work_unit_id: &str,
        command: WorkUnitCommand,
    ) -> Result<WorkUnitTransitionDecision, WorkUnitTransitionError> {
        let next_state = match (self, &command) {
            (Self::Pending(_), WorkUnitCommand::Activate) => {
                Self::Running(RunningWorkUnit::allocated(0, 1))
            }
            (Self::Running(value), WorkUnitCommand::Activate)
                if matches!(value.activity, RunningActivity::Allocated) =>
            {
                self.clone()
            }
            (
                Self::Running(value),
                WorkUnitCommand::StartTurn {
                    turn_id,
                    reset_budget,
                },
            ) => {
                if matches!(&value.activity, RunningActivity::Active { turn_id: active } if active == turn_id)
                {
                    self.clone()
                } else {
                    let mut value = value.clone();
                    value.activity = RunningActivity::Active {
                        turn_id: turn_id.clone(),
                    };
                    value.continuation = ExecutorContinuationState::idle(
                        value.continuation.revision().saturating_add(1),
                        if *reset_budget {
                            1
                        } else {
                            value.continuation.slice_count()
                        },
                    );
                    Self::Running(value)
                }
            }
            (
                Self::WaitingReview(WaitingReviewWorkUnit {
                    phase: WaitingReviewPhase::AwaitingReport(value),
                }),
                WorkUnitCommand::StartTurn {
                    turn_id,
                    reset_budget,
                },
            ) => Self::Running(RunningWorkUnit {
                activity: RunningActivity::Active {
                    turn_id: turn_id.clone(),
                },
                continuation: ExecutorContinuationState::idle(
                    value.continuation.revision().saturating_add(1),
                    if *reset_budget {
                        1
                    } else {
                        value.continuation.slice_count()
                    },
                ),
            }),
            (Self::ChangesRequired(value), WorkUnitCommand::StartTurn { turn_id, .. }) => {
                Self::Running(RunningWorkUnit {
                    activity: RunningActivity::Active {
                        turn_id: turn_id.clone(),
                    },
                    continuation: ExecutorContinuationState::idle(
                        value.continuation_revision.saturating_add(1),
                        value.slice_count,
                    ),
                })
            }
            (
                Self::Paused(PausedWorkUnit {
                    reason: WorkUnitPauseReason::Budget { .. },
                    continuation,
                }),
                WorkUnitCommand::StartTurn {
                    turn_id,
                    reset_budget: true,
                },
            ) => Self::Running(RunningWorkUnit {
                activity: RunningActivity::Active {
                    turn_id: turn_id.clone(),
                },
                continuation: ExecutorContinuationState::idle(
                    continuation.revision().saturating_add(1),
                    1,
                ),
            }),
            (
                Self::Running(RunningWorkUnit {
                    activity: RunningActivity::Active { turn_id },
                    ..
                }),
                WorkUnitCommand::FinishTurn { outcome },
            ) if turn_id != outcome.source_turn_id() => {
                return Err(WorkUnitTransitionError::CorrelationMismatch {
                    work_unit_id: work_unit_id.to_string(),
                    command: Box::new(command.clone()),
                    detail: format!(
                        "terminal outcome belongs to Turn {}, active Turn is {turn_id}",
                        outcome.source_turn_id()
                    ),
                });
            }
            (Self::Running(value), WorkUnitCommand::FinishTurn { outcome }) => {
                Self::WaitingReview(WaitingReviewWorkUnit {
                    phase: WaitingReviewPhase::AwaitingReport(AwaitingReport {
                        outcome: outcome.clone(),
                        continuation: ExecutorContinuationState::planner_wake_pending(
                            value.continuation.revision().saturating_add(1),
                            outcome.source_turn_id().to_string(),
                            value.continuation.slice_count(),
                        ),
                    }),
                })
            }
            (
                Self::Running(value),
                WorkUnitCommand::ContinueAfterBudget {
                    source_turn_id,
                    next_slice,
                    limit,
                },
            ) if !running_source_matches(value, source_turn_id) => {
                return Err(WorkUnitTransitionError::CorrelationMismatch {
                    work_unit_id: work_unit_id.to_string(),
                    command: Box::new(command.clone()),
                    detail: format!(
                        "budget continuation belongs to Turn {source_turn_id}, active source is {}",
                        running_source_id(value).unwrap_or("none")
                    ),
                });
            }
            (
                Self::Running(value),
                WorkUnitCommand::ContinueAfterBudget {
                    source_turn_id,
                    next_slice,
                    limit,
                },
            ) => Self::Running(RunningWorkUnit {
                activity: RunningActivity::Allocated,
                continuation: ExecutorContinuationState::pending_start(
                    value.continuation.revision().saturating_add(1),
                    source_turn_id.clone(),
                    *next_slice,
                    *limit,
                ),
            }),
            (
                Self::Running(value),
                WorkUnitCommand::PauseForBudget {
                    source_turn_id,
                    limit,
                    detail,
                },
            ) if !running_source_matches(value, source_turn_id) => {
                return Err(WorkUnitTransitionError::CorrelationMismatch {
                    work_unit_id: work_unit_id.to_string(),
                    command: Box::new(command.clone()),
                    detail: format!(
                        "budget pause belongs to Turn {source_turn_id}, active source is {}",
                        running_source_id(value).unwrap_or("none")
                    ),
                });
            }
            (
                Self::Running(value),
                WorkUnitCommand::PauseForBudget {
                    source_turn_id,
                    limit,
                    detail,
                },
            ) => Self::Paused(PausedWorkUnit {
                reason: WorkUnitPauseReason::Budget { limit: *limit },
                continuation: ExecutorContinuationState::needs_attention(
                    value.continuation.revision().saturating_add(1),
                    source_turn_id.clone(),
                    value.continuation.slice_count(),
                    detail.clone(),
                ),
            }),
            (
                Self::Pending(_) | Self::Running(_) | Self::WaitingReview(_),
                WorkUnitCommand::PauseOperational {
                    operation_id,
                    detail,
                },
            ) => Self::Paused(PausedWorkUnit {
                reason: WorkUnitPauseReason::Operational {
                    operation_id: operation_id.clone(),
                    detail: detail.clone(),
                },
                continuation: ExecutorContinuationState::idle(0, 1),
            }),
            (
                Self::Paused(PausedWorkUnit {
                    reason:
                        WorkUnitPauseReason::Operational {
                            operation_id: current_operation_id,
                            detail: current_detail,
                        },
                    ..
                }),
                WorkUnitCommand::PauseOperational {
                    operation_id,
                    detail,
                },
            ) if current_operation_id == operation_id && current_detail == detail => self.clone(),
            (
                Self::Running(_),
                WorkUnitCommand::SubmitCompletion {
                    completion_id,
                    completion_revision,
                    verification_summary,
                },
            ) => Self::WaitingReview(WaitingReviewWorkUnit {
                phase: WaitingReviewPhase::Ready(ReadyForReview {
                    completion_id: completion_id.clone(),
                    completion_revision: *completion_revision,
                    verification_summary: verification_summary.clone(),
                }),
            }),
            (
                Self::WaitingReview(WaitingReviewWorkUnit {
                    phase: WaitingReviewPhase::Ready(value),
                }),
                WorkUnitCommand::BeginReview { review_round_id },
            ) => Self::WaitingReview(WaitingReviewWorkUnit {
                phase: WaitingReviewPhase::Reviewing(ReviewInProgress {
                    completion_id: value.completion_id.clone(),
                    completion_revision: value.completion_revision,
                    review_round_id: review_round_id.clone(),
                    verification_summary: value.verification_summary.clone(),
                }),
            }),
            (
                Self::WaitingReview(WaitingReviewWorkUnit {
                    phase: WaitingReviewPhase::Reviewing(value),
                }),
                WorkUnitCommand::RequireChanges { review_round_id },
            ) if value.review_round_id == *review_round_id => {
                Self::ChangesRequired(ChangesRequiredWorkUnit {
                    completion_id: value.completion_id.clone(),
                    completion_revision: value.completion_revision,
                    review_round_id: review_round_id.clone(),
                    continuation_revision: 0,
                    slice_count: 1,
                })
            }
            (
                Self::WaitingReview(WaitingReviewWorkUnit {
                    phase: WaitingReviewPhase::Reviewing(value),
                }),
                WorkUnitCommand::ReviewFailed { review_round_id },
            ) if value.review_round_id == *review_round_id => {
                Self::WaitingReview(WaitingReviewWorkUnit {
                    phase: WaitingReviewPhase::Ready(ReadyForReview {
                        completion_id: value.completion_id.clone(),
                        completion_revision: value.completion_revision,
                        verification_summary: value.verification_summary.clone(),
                    }),
                })
            }
            (
                Self::WaitingReview(WaitingReviewWorkUnit {
                    phase: WaitingReviewPhase::Reviewing(value),
                }),
                WorkUnitCommand::PassReview {
                    review_round_id,
                    outcome,
                },
            ) if value.review_round_id == *review_round_id => match outcome {
                ReviewPassedOutcome::Delivery => Self::ReviewPassed(ReviewPassedWorkUnit {
                    completion_id: value.completion_id.clone(),
                    completion_revision: value.completion_revision,
                    review_round_id: review_round_id.clone(),
                    outcome: *outcome,
                    verification_summary: value.verification_summary.clone(),
                }),
                ReviewPassedOutcome::NoDelivery => Self::Completed(CompletedWorkUnit {
                    outcome: WorkUnitCompletionOutcome::NoDelivery {
                        completion_id: value.completion_id.clone(),
                    },
                }),
            },
            (
                Self::WaitingReview(WaitingReviewWorkUnit {
                    phase: WaitingReviewPhase::Reviewing(value),
                }),
                WorkUnitCommand::RequireChanges { review_round_id }
                | WorkUnitCommand::ReviewFailed { review_round_id }
                | WorkUnitCommand::PassReview {
                    review_round_id, ..
                },
            ) if value.review_round_id != *review_round_id => {
                return Err(WorkUnitTransitionError::CorrelationMismatch {
                    work_unit_id: work_unit_id.to_string(),
                    command: Box::new(command.clone()),
                    detail: format!(
                        "review command belongs to round {review_round_id}, active round is {}",
                        value.review_round_id
                    ),
                });
            }
            (Self::ReviewPassed(_), WorkUnitCommand::CompleteMerge { merge_record_id }) => {
                Self::Completed(CompletedWorkUnit {
                    outcome: WorkUnitCompletionOutcome::Merged {
                        merge_record_id: merge_record_id.clone(),
                    },
                })
            }
            (_, WorkUnitCommand::FailSpawn { failure }) if !self.kind().is_terminal() => {
                Self::Failed(FailedWorkUnit {
                    failure: WorkUnitFailure::Spawn(failure.clone()),
                    worktree_disposition: if failure.needs_attention() {
                        TaskWorktreeDisposition::Protect
                    } else {
                        TaskWorktreeDisposition::CleanupRequested
                    },
                })
            }
            (
                _,
                WorkUnitCommand::FailExecution {
                    operation_id,
                    detail,
                    disposition,
                },
            ) if !self.kind().is_terminal() => Self::Failed(FailedWorkUnit {
                failure: WorkUnitFailure::Execution {
                    operation_id: operation_id.clone(),
                    detail: detail.clone(),
                },
                worktree_disposition: *disposition,
            }),
            (
                _,
                WorkUnitCommand::Cancel {
                    operation_id,
                    reason,
                    disposition,
                },
            ) if !self.kind().is_terminal() => Self::Cancelled(CancelledWorkUnit {
                operation_id: operation_id.clone(),
                reason: reason.clone(),
                worktree_disposition: *disposition,
            }),
            (Self::Failed(value), WorkUnitCommand::FailSpawn { failure })
                if value.failure == WorkUnitFailure::Spawn(failure.clone()) =>
            {
                self.clone()
            }
            (
                Self::Failed(FailedWorkUnit {
                    failure:
                        WorkUnitFailure::Execution {
                            operation_id: current,
                            detail: current_detail,
                        },
                    worktree_disposition: current_disposition,
                }),
                WorkUnitCommand::FailExecution {
                    operation_id,
                    detail,
                    disposition,
                },
            ) if current == operation_id
                && current_detail == detail
                && current_disposition == disposition =>
            {
                self.clone()
            }
            (
                Self::Cancelled(value),
                WorkUnitCommand::Cancel {
                    operation_id,
                    reason,
                    disposition,
                },
            ) if value.operation_id == *operation_id
                && value.reason == *reason
                && value.worktree_disposition == *disposition =>
            {
                self.clone()
            }
            _ => {
                return Err(WorkUnitTransitionError::IllegalTransition {
                    work_unit_id: work_unit_id.to_string(),
                    current: self.kind(),
                    command: Box::new(command),
                });
            }
        };
        let changed = next_state != *self;
        Ok(WorkUnitTransitionDecision {
            next_state,
            changed,
        })
    }

    pub(crate) fn continuation(&self) -> Option<&ExecutorContinuationState> {
        match self {
            Self::Running(value) => Some(&value.continuation),
            Self::Paused(value) => Some(&value.continuation),
            Self::WaitingReview(WaitingReviewWorkUnit {
                phase: WaitingReviewPhase::AwaitingReport(value),
            }) => Some(&value.continuation),
            _ => None,
        }
    }

    pub(crate) fn waiting_review_phase(&self) -> Option<&WaitingReviewPhase> {
        match self {
            Self::WaitingReview(value) => Some(&value.phase),
            _ => None,
        }
    }

    pub(crate) fn completion_outcome(&self) -> Option<&WorkUnitCompletionOutcome> {
        match self {
            Self::Completed(value) => Some(&value.outcome),
            _ => None,
        }
    }

    pub(crate) fn execution_error(&self) -> Option<&str> {
        match self {
            Self::WaitingReview(WaitingReviewWorkUnit {
                phase:
                    WaitingReviewPhase::AwaitingReport(AwaitingReport {
                        outcome:
                            ExecutorTerminalOutcome::Completed { detail, .. }
                            | ExecutorTerminalOutcome::Failed { detail, .. },
                        ..
                    }),
            }) => Some(detail),
            Self::Paused(PausedWorkUnit {
                reason: WorkUnitPauseReason::Operational { detail, .. },
                ..
            }) => Some(detail),
            Self::Paused(value) => value.continuation.detail(),
            Self::Failed(FailedWorkUnit {
                failure: WorkUnitFailure::Execution { detail, .. },
                ..
            }) => Some(detail),
            Self::Failed(FailedWorkUnit {
                failure: WorkUnitFailure::Spawn(failure),
                ..
            }) => Some(&failure.message),
            Self::Cancelled(value) => Some(&value.reason),
            _ => None,
        }
    }

    pub(crate) fn spawn_failure(&self) -> Option<&TaskSpawnFailure> {
        match self {
            Self::Failed(FailedWorkUnit {
                failure: WorkUnitFailure::Spawn(failure),
                ..
            }) => Some(failure),
            _ => None,
        }
    }

    pub(crate) fn budget_limit(&self) -> Option<&pl_protocol::BudgetLimitSnapshot> {
        match self {
            Self::Paused(PausedWorkUnit {
                reason: WorkUnitPauseReason::Budget { limit },
                ..
            }) => Some(limit),
            _ => self
                .continuation()
                .and_then(ExecutorContinuationState::budget_limit),
        }
    }

    pub(crate) fn worktree_disposition(&self) -> TaskWorktreeDisposition {
        match self {
            Self::Completed(_) => TaskWorktreeDisposition::CleanupRequested,
            Self::Failed(value) => value.worktree_disposition,
            Self::Cancelled(value) => value.worktree_disposition,
            _ => TaskWorktreeDisposition::Protect,
        }
    }
}

fn running_source_id(state: &RunningWorkUnit) -> Option<&str> {
    match &state.activity {
        RunningActivity::Active { turn_id } => Some(turn_id),
        RunningActivity::Allocated => state.continuation.source_turn_id(),
    }
}

fn running_source_matches(state: &RunningWorkUnit, source_turn_id: &str) -> bool {
    running_source_id(state) == Some(source_turn_id)
}

impl ExecutorTerminalOutcome {
    pub(crate) fn source_turn_id(&self) -> &str {
        match self {
            Self::Completed { source_turn_id, .. } | Self::Failed { source_turn_id, .. } => {
                source_turn_id
            }
        }
    }
}
