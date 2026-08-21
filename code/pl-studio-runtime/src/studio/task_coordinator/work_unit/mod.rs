//! WorkUnit aggregate with a single lifecycle state.

use std::ops::Deref;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::{
    ExecutorContinuationState, TaskWorktreeDisposition, ThreadExecutionStatus, WorkUnitStatus,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkUnitProgress {
    pub(crate) worktree_disposition: TaskWorktreeDisposition,
    pub(crate) execution_summary: Option<String>,
    pub(crate) execution_error: Option<String>,
    pub(crate) budget_limit: Option<pl_protocol::BudgetLimitSnapshot>,
    pub(crate) budget_slice_count: u32,
    pub(crate) continuation_state: ExecutorContinuationState,
    pub(crate) continuation_source_turn_id: Option<String>,
    pub(crate) continuation_revision: u64,
}

impl WorkUnitProgress {
    pub(crate) fn pending() -> Self {
        Self {
            worktree_disposition: TaskWorktreeDisposition::Protect,
            execution_summary: None,
            execution_error: None,
            budget_limit: None,
            budget_slice_count: 1,
            continuation_state: ExecutorContinuationState::None,
            continuation_source_turn_id: None,
            continuation_revision: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum RunningExecution {
    Running,
    BudgetLimited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum AwaitingExecution {
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunningWorkUnitState {
    pub(crate) execution: RunningExecution,
    pub(crate) progress: WorkUnitProgress,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AwaitingWorkUnitState {
    pub(crate) execution: AwaitingExecution,
    pub(crate) progress: WorkUnitProgress,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub(crate) enum WorkUnitState {
    Pending(WorkUnitProgress),
    Running(RunningWorkUnitState),
    AwaitingCompletion(AwaitingWorkUnitState),
    ReadyForReview(WorkUnitProgress),
    Reviewing(WorkUnitProgress),
    ChangesRequested(WorkUnitProgress),
    Approved(WorkUnitProgress),
    Merged(WorkUnitProgress),
    NoDelivery(WorkUnitProgress),
    NeedsAttention(WorkUnitProgress),
    Failed(WorkUnitProgress),
    Cancelled(WorkUnitProgress),
}

impl WorkUnitState {
    pub(crate) fn pending() -> Self {
        Self::Pending(WorkUnitProgress::pending())
    }

    pub(crate) fn running(progress: WorkUnitProgress) -> Self {
        Self::Running(RunningWorkUnitState {
            execution: RunningExecution::Running,
            progress,
        })
    }

    pub(crate) fn budget_limited(progress: WorkUnitProgress) -> Self {
        Self::Running(RunningWorkUnitState {
            execution: RunningExecution::BudgetLimited,
            progress,
        })
    }

    pub(crate) fn awaiting_completed(progress: WorkUnitProgress) -> Self {
        Self::AwaitingCompletion(AwaitingWorkUnitState {
            execution: AwaitingExecution::Completed,
            progress,
        })
    }

    pub(crate) fn awaiting_failed(progress: WorkUnitProgress) -> Self {
        Self::AwaitingCompletion(AwaitingWorkUnitState {
            execution: AwaitingExecution::Failed,
            progress,
        })
    }

    pub(crate) fn awaiting_cancelled(progress: WorkUnitProgress) -> Self {
        Self::AwaitingCompletion(AwaitingWorkUnitState {
            execution: AwaitingExecution::Cancelled,
            progress,
        })
    }

    pub(crate) fn ready_for_review(progress: WorkUnitProgress) -> Self {
        Self::ReadyForReview(progress)
    }

    pub(crate) fn reviewing(progress: WorkUnitProgress) -> Self {
        Self::Reviewing(progress)
    }

    pub(crate) fn changes_requested(progress: WorkUnitProgress) -> Self {
        Self::ChangesRequested(progress)
    }

    pub(crate) fn approved(progress: WorkUnitProgress) -> Self {
        Self::Approved(progress)
    }

    pub(crate) fn merged(progress: WorkUnitProgress) -> Self {
        Self::Merged(progress)
    }

    pub(crate) fn no_delivery(progress: WorkUnitProgress) -> Self {
        Self::NoDelivery(progress)
    }

    pub(crate) fn needs_attention(progress: WorkUnitProgress) -> Self {
        Self::NeedsAttention(progress)
    }

    pub(crate) fn failed(progress: WorkUnitProgress) -> Self {
        Self::Failed(progress)
    }

    pub(crate) fn cancelled(progress: WorkUnitProgress) -> Self {
        Self::Cancelled(progress)
    }

    pub(crate) const fn status(&self) -> WorkUnitStatus {
        match self {
            Self::Pending(_) => WorkUnitStatus::Pending,
            Self::Running(_) => WorkUnitStatus::Running,
            Self::AwaitingCompletion(_) => WorkUnitStatus::AwaitingCompletion,
            Self::ReadyForReview(_) => WorkUnitStatus::ReadyForReview,
            Self::Reviewing(_) => WorkUnitStatus::Reviewing,
            Self::ChangesRequested(_) => WorkUnitStatus::ChangesRequested,
            Self::Approved(_) => WorkUnitStatus::Approved,
            Self::Merged(_) => WorkUnitStatus::Merged,
            Self::NoDelivery(_) => WorkUnitStatus::NoDelivery,
            Self::NeedsAttention(_) => WorkUnitStatus::NeedsAttention,
            Self::Failed(_) => WorkUnitStatus::Failed,
            Self::Cancelled(_) => WorkUnitStatus::Cancelled,
        }
    }

    pub(crate) const fn execution_status(&self) -> ThreadExecutionStatus {
        match self {
            Self::Pending(_) => ThreadExecutionStatus::Queued,
            Self::Running(state) => match state.execution {
                RunningExecution::Running => ThreadExecutionStatus::Running,
                RunningExecution::BudgetLimited => ThreadExecutionStatus::BudgetLimited,
            },
            Self::AwaitingCompletion(state) => match state.execution {
                AwaitingExecution::Completed => ThreadExecutionStatus::Completed,
                AwaitingExecution::Failed => ThreadExecutionStatus::Failed,
                AwaitingExecution::Cancelled => ThreadExecutionStatus::Cancelled,
            },
            Self::ReadyForReview(_)
            | Self::Reviewing(_)
            | Self::ChangesRequested(_)
            | Self::Approved(_)
            | Self::Merged(_)
            | Self::NoDelivery(_) => ThreadExecutionStatus::Completed,
            Self::NeedsAttention(_) => ThreadExecutionStatus::BudgetLimited,
            Self::Failed(_) => ThreadExecutionStatus::Failed,
            Self::Cancelled(_) => ThreadExecutionStatus::Cancelled,
        }
    }

    pub(crate) fn progress(&self) -> &WorkUnitProgress {
        match self {
            Self::Pending(progress)
            | Self::ReadyForReview(progress)
            | Self::Reviewing(progress)
            | Self::ChangesRequested(progress)
            | Self::Approved(progress)
            | Self::Merged(progress)
            | Self::NoDelivery(progress)
            | Self::NeedsAttention(progress)
            | Self::Failed(progress)
            | Self::Cancelled(progress) => progress,
            Self::Running(state) => &state.progress,
            Self::AwaitingCompletion(state) => &state.progress,
        }
    }

    pub(crate) fn into_progress(self) -> WorkUnitProgress {
        match self {
            Self::Pending(progress)
            | Self::ReadyForReview(progress)
            | Self::Reviewing(progress)
            | Self::ChangesRequested(progress)
            | Self::Approved(progress)
            | Self::Merged(progress)
            | Self::NoDelivery(progress)
            | Self::NeedsAttention(progress)
            | Self::Failed(progress)
            | Self::Cancelled(progress) => progress,
            Self::Running(state) => state.progress,
            Self::AwaitingCompletion(state) => state.progress,
        }
    }

    /// 替换状态 payload，同时保留当前 lifecycle variant 与 execution variant。
    pub(crate) fn with_progress(self, progress: WorkUnitProgress) -> Self {
        match self {
            Self::Pending(_) => Self::Pending(progress),
            Self::Running(mut state) => {
                state.progress = progress;
                Self::Running(state)
            }
            Self::AwaitingCompletion(mut state) => {
                state.progress = progress;
                Self::AwaitingCompletion(state)
            }
            Self::ReadyForReview(_) => Self::ReadyForReview(progress),
            Self::Reviewing(_) => Self::Reviewing(progress),
            Self::ChangesRequested(_) => Self::ChangesRequested(progress),
            Self::Approved(_) => Self::Approved(progress),
            Self::Merged(_) => Self::Merged(progress),
            Self::NoDelivery(_) => Self::NoDelivery(progress),
            Self::NeedsAttention(_) => Self::NeedsAttention(progress),
            Self::Failed(_) => Self::Failed(progress),
            Self::Cancelled(_) => Self::Cancelled(progress),
        }
    }
}

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
    pub(crate) const fn status(&self) -> WorkUnitStatus {
        self.state.status()
    }

    pub(crate) const fn execution_status(&self) -> ThreadExecutionStatus {
        self.state.execution_status()
    }

    pub(crate) fn progress(&self) -> &WorkUnitProgress {
        self.state.progress()
    }

    pub(crate) fn worktree_disposition(&self) -> TaskWorktreeDisposition {
        self.progress().worktree_disposition
    }

    pub(crate) fn execution_summary(&self) -> Option<&str> {
        self.progress().execution_summary.as_deref()
    }

    pub(crate) fn execution_error(&self) -> Option<&str> {
        self.progress().execution_error.as_deref()
    }

    pub(crate) fn budget_limit(&self) -> Option<&pl_protocol::BudgetLimitSnapshot> {
        self.progress().budget_limit.as_ref()
    }

    pub(crate) fn budget_slice_count(&self) -> u32 {
        self.progress().budget_slice_count
    }

    pub(crate) fn continuation_state(&self) -> ExecutorContinuationState {
        self.progress().continuation_state
    }

    pub(crate) fn continuation_source_turn_id(&self) -> Option<&str> {
        self.progress().continuation_source_turn_id.as_deref()
    }

    pub(crate) fn continuation_revision(&self) -> u64 {
        self.progress().continuation_revision
    }
}

pub(crate) fn decode_work_unit_state(value: &str) -> Result<WorkUnitState> {
    serde_json::from_str(value).context("invalid stored WorkUnit state JSON")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_round_trip_as_a_single_tagged_enum() {
        let states = [
            WorkUnitState::pending(),
            WorkUnitState::running(WorkUnitProgress::pending()),
            WorkUnitState::budget_limited(WorkUnitProgress::pending()),
            WorkUnitState::awaiting_completed(WorkUnitProgress::pending()),
            WorkUnitState::awaiting_failed(WorkUnitProgress::pending()),
            WorkUnitState::awaiting_cancelled(WorkUnitProgress::pending()),
            WorkUnitState::ready_for_review(WorkUnitProgress::pending()),
            WorkUnitState::reviewing(WorkUnitProgress::pending()),
            WorkUnitState::changes_requested(WorkUnitProgress::pending()),
            WorkUnitState::approved(WorkUnitProgress::pending()),
            WorkUnitState::merged(WorkUnitProgress::pending()),
            WorkUnitState::no_delivery(WorkUnitProgress::pending()),
            WorkUnitState::needs_attention(WorkUnitProgress::pending()),
            WorkUnitState::failed(WorkUnitProgress::pending()),
            WorkUnitState::cancelled(WorkUnitProgress::pending()),
        ];

        for state in states {
            let value = serde_json::to_value(&state).unwrap();
            assert_eq!(value["kind"], state.status().as_str());
            let decoded: WorkUnitState = serde_json::from_value(value).unwrap();
            assert_eq!(decoded, state);
        }
    }
}
