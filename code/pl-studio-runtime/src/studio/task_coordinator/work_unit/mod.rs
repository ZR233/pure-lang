//! WorkUnit aggregate and its command-driven lifecycle state machine.

mod state;

use std::ops::Deref;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub(crate) use state::*;

use super::{TaskSpawnFailure, TaskWorktreeDisposition};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkUnitContext {
    pub(crate) id: String,
    pub(crate) task_run_id: String,
    pub(crate) title: String,
    pub(crate) scope_hints: Vec<String>,
    pub(crate) base_commit: String,
    pub(crate) worktree_path: String,
    pub(crate) branch: String,
    pub(crate) attempt: u32,
    pub(crate) executor_thread_id: Option<String>,
    pub(crate) requested_by_call_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkUnit {
    pub(crate) context: WorkUnitContext,
    pub(crate) state: WorkUnitState,
    pub(crate) revision: u64,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
}

impl Deref for WorkUnit {
    type Target = WorkUnitContext;

    fn deref(&self) -> &Self::Target {
        &self.context
    }
}

impl WorkUnit {
    pub(crate) const fn kind(&self) -> WorkUnitStateKind {
        self.state.kind()
    }

    pub(crate) fn decide(
        &self,
        expected_revision: u64,
        command: WorkUnitCommand,
    ) -> std::result::Result<WorkUnitTransitionDecision, WorkUnitTransitionError> {
        if expected_revision != self.revision {
            return Err(WorkUnitTransitionError::StaleRevision {
                work_unit_id: self.id.clone(),
                expected: expected_revision,
                actual: self.revision,
                command: Box::new(command),
            });
        }
        self.state.decide(&self.id, command)
    }

    pub(crate) fn worktree_disposition(&self) -> TaskWorktreeDisposition {
        self.state.worktree_disposition()
    }

    pub(crate) fn execution_error(&self) -> Option<&str> {
        self.state.execution_error()
    }

    pub(crate) fn spawn_failure(&self) -> Option<&TaskSpawnFailure> {
        self.state.spawn_failure()
    }

    pub(crate) fn budget_limit(&self) -> Option<&pl_protocol::BudgetLimitSnapshot> {
        self.state.budget_limit()
    }

    pub(crate) fn budget_slice_count(&self) -> u32 {
        match &self.state {
            WorkUnitState::ChangesRequired(value) => value.slice_count(),
            _ => self
                .state
                .continuation()
                .map_or(1, ExecutorContinuationState::slice_count),
        }
    }

    pub(crate) fn continuation_state(&self) -> ExecutorContinuationStateKind {
        self.state.continuation().map_or(
            ExecutorContinuationStateKind::Idle,
            ExecutorContinuationState::kind,
        )
    }

    pub(crate) fn continuation_source_turn_id(&self) -> Option<&str> {
        self.state
            .continuation()
            .and_then(ExecutorContinuationState::source_turn_id)
    }

    pub(crate) fn continuation_revision(&self) -> u64 {
        match &self.state {
            WorkUnitState::ChangesRequired(value) => value.continuation_revision(),
            _ => self
                .state
                .continuation()
                .map_or(0, ExecutorContinuationState::revision),
        }
    }

    pub(crate) fn waiting_review_phase(&self) -> Option<&WaitingReviewPhase> {
        self.state.waiting_review_phase()
    }

    pub(crate) fn completion_outcome(&self) -> Option<&WorkUnitCompletionOutcome> {
        self.state.completion_outcome()
    }
}

pub(crate) fn decode_work_unit_state(value: &str) -> Result<WorkUnitState> {
    serde_json::from_str(value).context("invalid stored WorkUnit state JSON")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_protocol::{BudgetLimitKind, BudgetLimitSnapshot, BudgetUsage};

    fn work_unit(state: WorkUnitState, revision: u64) -> WorkUnit {
        WorkUnit {
            context: WorkUnitContext {
                id: "work-1".to_string(),
                task_run_id: "task-1".to_string(),
                title: "work".to_string(),
                scope_hints: Vec::new(),
                base_commit: "base".to_string(),
                worktree_path: "path".to_string(),
                branch: "branch".to_string(),
                attempt: 1,
                executor_thread_id: None,
                requested_by_call_id: "call-1".to_string(),
            },
            state,
            revision,
            created_at: 1,
            updated_at: 1,
        }
    }

    fn next(state: &WorkUnitState, command: WorkUnitCommand) -> WorkUnitState {
        state.decide("work-1", command).unwrap().next_state()
    }

    fn budget_limit() -> BudgetLimitSnapshot {
        BudgetLimitSnapshot {
            kind: BudgetLimitKind::ModelStep,
            usage: BudgetUsage {
                model_steps: 10,
                ..BudgetUsage::default()
            },
        }
    }

    #[test]
    fn states_round_trip_as_a_single_tagged_enum() {
        let pending = WorkUnitState::pending();
        let running = next(&pending, WorkUnitCommand::Activate);
        let waiting_review = next(
            &running,
            WorkUnitCommand::SubmitCompletion {
                completion_id: "completion-1".to_string(),
                completion_revision: 1,
                verification_summary: "verified".to_string(),
            },
        );
        let reviewing = next(
            &waiting_review,
            WorkUnitCommand::BeginReview {
                review_round_id: "review-1".to_string(),
            },
        );
        let review_passed = next(
            &reviewing,
            WorkUnitCommand::PassReview {
                review_round_id: "review-1".to_string(),
                outcome: ReviewPassedOutcome::Delivery,
            },
        );
        let changes_required = next(
            &reviewing,
            WorkUnitCommand::RequireChanges {
                review_round_id: "review-1".to_string(),
            },
        );
        let paused = next(
            &running,
            WorkUnitCommand::PauseOperational {
                operation_id: "pause-1".to_string(),
                detail: "operator attention".to_string(),
            },
        );
        let completed = next(
            &review_passed,
            WorkUnitCommand::CompleteMerge {
                merge_record_id: "merge-1".to_string(),
            },
        );
        let failed = next(
            &running,
            WorkUnitCommand::FailExecution {
                operation_id: "failure-1".to_string(),
                detail: "failed".to_string(),
                disposition: TaskWorktreeDisposition::Protect,
            },
        );
        let cancelled = next(
            &running,
            WorkUnitCommand::Cancel {
                operation_id: "cancel-1".to_string(),
                reason: "cancelled".to_string(),
                disposition: TaskWorktreeDisposition::CleanupRequested,
            },
        );
        let states = [
            pending,
            running,
            waiting_review,
            review_passed,
            changes_required,
            paused,
            completed,
            failed,
            cancelled,
        ];

        for state in states {
            let value = serde_json::to_value(&state).unwrap();
            assert_eq!(value["kind"], state.kind().as_str());
            let decoded: WorkUnitState = serde_json::from_value(value).unwrap();
            assert_eq!(decoded, state);
        }
    }

    #[test]
    fn terminal_state_rejects_non_idempotent_commands() {
        let pending = work_unit(WorkUnitState::pending(), 0);
        let cancel = WorkUnitCommand::Cancel {
            operation_id: "cancel-1".to_string(),
            reason: "stop".to_string(),
            disposition: TaskWorktreeDisposition::CleanupRequested,
        };
        let cancelled = pending.decide(0, cancel.clone()).unwrap().next_state();
        let replay = cancelled.decide("work-1", cancel).unwrap();
        assert!(!replay.changed());
        assert_eq!(replay.next_state(), cancelled);
        assert!(
            cancelled
                .decide("work-1", WorkUnitCommand::Activate)
                .is_err()
        );
    }

    #[test]
    fn aggregate_rejects_stale_revision() {
        let work_unit = work_unit(WorkUnitState::pending(), 3);
        assert!(matches!(
            work_unit.decide(2, WorkUnitCommand::Activate),
            Err(WorkUnitTransitionError::StaleRevision {
                expected: 2,
                actual: 3,
                ..
            })
        ));
    }

    #[test]
    fn budget_commands_require_the_active_source_turn() {
        let running = next(&WorkUnitState::pending(), WorkUnitCommand::Activate);
        let active = next(
            &running,
            WorkUnitCommand::StartTurn {
                turn_id: "turn-1".to_string(),
                reset_budget: false,
            },
        );
        let command = WorkUnitCommand::ContinueAfterBudget {
            source_turn_id: "turn-2".to_string(),
            next_slice: 2,
            limit: budget_limit(),
        };
        assert!(matches!(
            active.decide("work-1", command),
            Err(WorkUnitTransitionError::CorrelationMismatch { .. })
        ));

        let pending_start = next(
            &active,
            WorkUnitCommand::ContinueAfterBudget {
                source_turn_id: "turn-1".to_string(),
                next_slice: 2,
                limit: budget_limit(),
            },
        );
        assert!(matches!(
            pending_start.decide(
                "work-1",
                WorkUnitCommand::PauseForBudget {
                    source_turn_id: "turn-2".to_string(),
                    limit: budget_limit(),
                    detail: "continuation failed".to_string(),
                },
            ),
            Err(WorkUnitTransitionError::CorrelationMismatch { .. })
        ));
    }

    #[test]
    fn operational_pause_replays_only_for_the_same_operation() {
        let command = WorkUnitCommand::PauseOperational {
            operation_id: "pause-1".to_string(),
            detail: "attention".to_string(),
        };
        let paused = next(&WorkUnitState::pending(), command.clone());
        let replay = paused.decide("work-1", command).unwrap();
        assert!(!replay.changed());
        assert!(matches!(
            paused.decide(
                "work-1",
                WorkUnitCommand::PauseOperational {
                    operation_id: "pause-2".to_string(),
                    detail: "attention".to_string(),
                },
            ),
            Err(WorkUnitTransitionError::IllegalTransition { .. })
        ));
    }

    #[test]
    fn legacy_or_incomplete_state_json_is_rejected() {
        for legacy in [
            r#"{"status":"running"}"#,
            r#"{"kind":"running"}"#,
            r#"{"kind":"cancelled","data":{"operationId":"cancel-1","reason":"stop"}}"#,
        ] {
            assert!(decode_work_unit_state(legacy).is_err(), "accepted {legacy}");
        }
    }
}
