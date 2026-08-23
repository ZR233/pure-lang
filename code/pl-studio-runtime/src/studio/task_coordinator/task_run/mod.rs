//! TaskRun 聚合：一条记录承载从拟定计划到完成的完整生命周期。

mod state;

use std::ops::Deref;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

pub(crate) use state::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskPlan {
    pub(crate) content: String,
    pub(crate) revision: u64,
    pub(crate) submitted_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskContext {
    pub(crate) id: String,
    pub(crate) project_id: String,
    pub(crate) root_thread_id: String,
    pub(crate) request: String,
    pub(crate) plan: Option<TaskPlan>,
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

    pub(crate) fn generation(&self) -> u64 {
        self.state.generation()
    }

    pub(crate) fn plan_content(&self) -> Option<&str> {
        self.plan.as_ref().map(|plan| plan.content.as_str())
    }

    pub(crate) fn decide(&self, command: TaskCommand) -> Result<TransitionDecision> {
        let next_state = match command {
            TaskCommand::SubmitPlan { plan_revision } => {
                self.state.clone().submit_plan(plan_revision)?
            }
            TaskCommand::ConfirmPlan { plan_revision } => {
                self.state.clone().confirm_plan(plan_revision)?
            }
            TaskCommand::RequestPlanRevision => self.state.clone().request_plan_revision()?,
            TaskCommand::FinishDocumentEditing { summary } => {
                self.state.clone().finish_document_editing(summary)?
            }
            TaskCommand::BeginIntegratedReview { target } => {
                self.state.clone().begin_review(target)?
            }
            TaskCommand::ReturnToWorking { summary } => {
                self.state.clone().return_to_working(summary)?
            }
            TaskCommand::Stop => self.state.clone().advance_generation()?,
            TaskCommand::Complete { outcome } => self.state.clone().complete(outcome)?,
        };
        if self.kind().is_terminal() {
            bail!("completed task is immutable");
        }
        Ok(TransitionDecision { next_state })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TaskCommand {
    SubmitPlan { plan_revision: u64 },
    ConfirmPlan { plan_revision: u64 },
    RequestPlanRevision,
    FinishDocumentEditing { summary: String },
    BeginIntegratedReview { target: IntegratedReviewTarget },
    ReturnToWorking { summary: String },
    Stop,
    Complete { outcome: TaskOutcome },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TransitionDecision {
    pub(crate) next_state: TaskRunState,
}
