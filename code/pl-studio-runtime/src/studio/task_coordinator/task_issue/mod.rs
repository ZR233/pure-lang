//! Task issue 聚合；失败内容、处置语义与解决时间只存在于 canonical state。

mod state;

use serde::Serialize;

pub(crate) use state::{
    TaskIssueCommand, TaskIssueDisposition, TaskIssueState, TaskIssueStateKind,
    TaskIssueTransitionDecision, TaskIssueTransitionError,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskIssueRecord {
    pub(crate) id: String,
    pub(crate) task_run_id: String,
    pub(crate) source_thread_id: String,
    pub(crate) source_turn_id: String,
    pub(crate) source_agent_id: String,
    pub(crate) source_role: String,
    pub(crate) work_unit_id: Option<String>,
    pub(crate) review_round_id: Option<String>,
    pub(crate) state: TaskIssueState,
    pub(crate) revision: u64,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
}

impl TaskIssueRecord {
    pub(crate) fn failure(&self) -> &pl_protocol::TurnFailure {
        self.state.failure()
    }

    pub(crate) fn decide(
        &self,
        expected_revision: u64,
        command: TaskIssueCommand,
    ) -> Result<TaskIssueTransitionDecision, TaskIssueTransitionError> {
        if expected_revision != self.revision {
            return Err(TaskIssueTransitionError::StaleRevision {
                task_issue_id: self.id.clone(),
                expected: expected_revision,
                actual: self.revision,
                command,
            });
        }
        self.state.decide(&self.id, command)
    }
}

pub(crate) struct RecordTaskAgentFailure {
    pub(crate) root_thread_id: String,
    pub(crate) source_thread_id: String,
    pub(crate) source_turn_id: String,
    pub(crate) source_agent_id: String,
    pub(crate) source_role: String,
    pub(crate) failure: pl_protocol::TurnFailure,
}

pub(crate) struct TaskIssueSettlement {
    pub(crate) terminalized: bool,
}
