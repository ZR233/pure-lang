//! TaskRun aggregate and lifecycle state machine.

mod state;
use std::ops::Deref;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

pub(crate) use state::*;

use super::{TaskStopOrigin, TaskStopReason};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskContext {
    pub(crate) id: String,
    pub(crate) project_id: String,
    pub(crate) root_thread_id: String,
    pub(crate) plan: String,
    pub(crate) workspace_root: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskRun {
    pub(crate) context: TaskContext,
    pub(crate) state: TaskRunState,
    pub(crate) revision: u64,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
}

impl Deref for TaskRun {
    type Target = TaskContext;

    fn deref(&self) -> &Self::Target {
        &self.context
    }
}

impl TaskRun {
    pub(crate) fn kind(&self) -> TaskRunStateKind {
        self.state.kind()
    }

    pub(crate) fn status_message(&self) -> Option<&str> {
        self.state.status_message()
    }

    pub(crate) fn is_stop_requested(&self) -> bool {
        self.stop_request().is_some()
    }

    pub(crate) fn generation(&self) -> u64 {
        self.state.generation()
    }

    pub(crate) fn terminal_generation(&self) -> Option<u64> {
        self.kind().is_terminal().then(|| self.generation())
    }

    pub(crate) fn stop_reason(&self) -> Option<&TaskStopReason> {
        self.stop_request().map(|request| &request.reason)
    }

    #[cfg(test)]
    pub(crate) fn design_summary(&self) -> Option<&str> {
        self.design().map(|design| design.summary.as_str())
    }

    pub(crate) fn design(&self) -> Option<&FinalizedDesign> {
        self.state.design().finalized()
    }

    pub(crate) fn stop_request(&self) -> Option<&TaskStopRequest> {
        self.state.stop_request()
    }

    pub(crate) fn terminal_failure_id(&self) -> Option<&str> {
        self.state.terminal_failure_id()
    }

    pub(crate) fn decide(&self, command: TaskCommand) -> Result<TransitionDecision> {
        self.state.clone().decide(command)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TaskCommand {
    FinalizeDesign(FinalizedDesign),
    BeginImplementing,
    BeginMerging {
        status_message: Option<String>,
    },
    BeginReviewing(ReviewTarget),
    BeginReworking {
        status_message: String,
    },
    RequestStop(TaskStopRequest),
    Block {
        message: String,
        recovery: BlockedRecovery,
    },
    RecoverBlocked {
        recovery: BlockedRecovery,
        status_message: String,
    },
    Complete,
    Fail {
        message: String,
        failure_id: Option<String>,
    },
    Cancel {
        message: String,
        request: Option<TaskStopRequest>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TransitionDecision {
    pub(crate) next_state: TaskRunState,
    pub(crate) durable_effects: Vec<TaskDurableEffect>,
    pub(crate) external_effects: Vec<TaskExternalEffect>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TaskDurableEffect {
    ReleaseProjectLease,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TaskExternalEffect {
    InterruptAgents,
}

impl TransitionDecision {
    fn state(next_state: TaskRunState) -> Self {
        Self {
            next_state,
            durable_effects: Vec::new(),
            external_effects: Vec::new(),
        }
    }

    fn stopping(next_state: TaskRunState) -> Self {
        Self {
            next_state,
            durable_effects: Vec::new(),
            external_effects: vec![TaskExternalEffect::InterruptAgents],
        }
    }

    fn terminal(next_state: TaskRunState) -> Self {
        Self {
            next_state,
            durable_effects: vec![TaskDurableEffect::ReleaseProjectLease],
            external_effects: Vec::new(),
        }
    }
}

impl TaskRunState {
    pub(crate) fn decide(self, command: TaskCommand) -> Result<TransitionDecision> {
        let generation = self.generation();
        let design = self.design().clone();
        match (self, command) {
            (Self::DesignUpdating(_), TaskCommand::FinalizeDesign(design)) => {
                Ok(TransitionDecision::state(Self::Implementing(
                    ImplementingState::new(design, generation),
                )))
            }
            (Self::Merging(_) | Self::Reworking(_), TaskCommand::BeginImplementing) => {
                Ok(TransitionDecision::state(Self::Implementing(
                    ImplementingState::new(require_finalized_design(design)?, generation),
                )))
            }
            (
                Self::Implementing(_) | Self::Reworking(_),
                TaskCommand::BeginMerging { status_message },
            ) => Ok(TransitionDecision::state(Self::Merging(MergingState::new(
                require_finalized_design(design)?,
                generation,
                status_message,
            )))),
            (
                Self::Implementing(_) | Self::Merging(_) | Self::Reworking(_),
                TaskCommand::BeginReviewing(target),
            ) => Ok(TransitionDecision::state(Self::Reviewing(
                ReviewingState::new(require_finalized_design(design)?, generation, target),
            ))),
            (
                Self::Reviewing(_) | Self::Merging(_),
                TaskCommand::BeginReworking { status_message },
            ) => Ok(TransitionDecision::state(Self::Reworking(
                ReworkingState::new(
                    require_finalized_design(design)?,
                    generation,
                    status_message,
                ),
            ))),
            (state, TaskCommand::RequestStop(request)) if !state.kind().is_terminal() => {
                Ok(TransitionDecision::stopping(Self::Stopping(
                    StoppingState::new(design, next_generation(generation)?, request),
                )))
            }
            (state, TaskCommand::Block { message, recovery }) if !state.kind().is_terminal() => {
                Ok(TransitionDecision::state(Self::Blocked(BlockedState::new(
                    design, generation, message, recovery,
                ))))
            }
            (
                Self::Blocked(blocked),
                TaskCommand::RecoverBlocked {
                    recovery: BlockedRecovery::RetryMerge,
                    ..
                },
            ) if blocked.recovery() == &BlockedRecovery::RetryMerge => {
                Ok(TransitionDecision::state(Self::Merging(MergingState::new(
                    require_finalized_design(blocked.design().clone())?,
                    next_generation(blocked.generation())?,
                    None,
                ))))
            }
            (
                Self::Blocked(blocked),
                TaskCommand::RecoverBlocked {
                    recovery: BlockedRecovery::ResumeRework,
                    status_message,
                },
            ) if blocked.recovery() == &BlockedRecovery::ResumeRework => Ok(
                TransitionDecision::state(Self::Reworking(ReworkingState::new(
                    require_finalized_design(blocked.design().clone())?,
                    next_generation(blocked.generation())?,
                    status_message,
                ))),
            ),
            (
                Self::Implementing(_) | Self::Reworking(_) | Self::Reviewing(_),
                TaskCommand::Complete,
            ) => Ok(TransitionDecision::terminal(Self::Completed(
                CompletedState::new(require_finalized_design(design)?, generation),
            ))),
            (
                state,
                TaskCommand::Fail {
                    message,
                    failure_id,
                },
            ) if !state.kind().is_terminal() => Ok(TransitionDecision::terminal(Self::Failed(
                FailedState::new(design, generation, message, failure_id),
            ))),
            (state, TaskCommand::Cancel { message, request })
                if !matches!(
                    state.kind(),
                    TaskRunStateKind::Completed
                        | TaskRunStateKind::Failed
                        | TaskRunStateKind::Cancelled
                ) =>
            {
                Ok(TransitionDecision::terminal(Self::Cancelled(
                    CancelledState::new(design, generation, message, request),
                )))
            }
            (state, command) => bail!(
                "invalid task state command: {} cannot handle {}",
                state.kind().as_str(),
                command.name()
            ),
        }
    }
}

impl TaskCommand {
    fn name(&self) -> &'static str {
        match self {
            Self::FinalizeDesign(_) => "finalizeDesign",
            Self::BeginImplementing => "beginImplementing",
            Self::BeginMerging { .. } => "beginMerging",
            Self::BeginReviewing(_) => "beginReviewing",
            Self::BeginReworking { .. } => "beginReworking",
            Self::RequestStop(_) => "requestStop",
            Self::Block { .. } => "block",
            Self::RecoverBlocked { .. } => "recoverBlocked",
            Self::Complete => "complete",
            Self::Fail { .. } => "fail",
            Self::Cancel { .. } => "cancel",
        }
    }
}

fn next_generation(generation: u64) -> Result<u64> {
    generation
        .checked_add(1)
        .context("task generation overflow")
}

fn require_finalized_design(design: DesignProgress) -> Result<FinalizedDesign> {
    match design {
        DesignProgress::Updating => bail!("task design has not been finalized"),
        DesignProgress::Finalized(design) => Ok(*design),
    }
}

impl From<(TaskStopOrigin, TaskStopReason, i64)> for TaskStopRequest {
    fn from((origin, reason, requested_at): (TaskStopOrigin, TaskStopReason, i64)) -> Self {
        Self {
            origin,
            reason,
            requested_at,
        }
    }
}

#[cfg(test)]
mod unit_tests;
