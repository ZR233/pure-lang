mod content;
mod lifecycle;

use serde::Serialize;

pub(crate) use content::{WorkCompletionContent, WorkCompletionKind};
#[cfg(test)]
pub(crate) use lifecycle::ReviewedCompletion;
pub(crate) use lifecycle::{
    WorkCompletionCommand, WorkCompletionState, WorkCompletionStatus,
    WorkCompletionTransitionDecision, WorkCompletionTransitionError,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkCompletionRecord {
    pub(crate) id: String,
    pub(crate) task_run_id: String,
    pub(crate) work_unit_id: String,
    pub(crate) executor_agent_id: String,
    pub(crate) revision: u32,
    pub(crate) content: WorkCompletionContent,
    pub(crate) state: WorkCompletionState,
    pub(crate) state_revision: u64,
    pub(crate) base_commit: String,
    pub(crate) verification_summary: String,
    pub(crate) worktree_path: String,
    pub(crate) branch: String,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
}

impl WorkCompletionRecord {
    pub(crate) const fn kind(&self) -> WorkCompletionKind {
        self.content.kind()
    }

    pub(crate) const fn status(&self) -> WorkCompletionStatus {
        self.state.status()
    }

    pub(crate) fn head_commit(&self) -> Option<&str> {
        self.content.head_commit()
    }

    pub(crate) fn changed_files(&self) -> &[String] {
        self.content.changed_files()
    }

    pub(crate) fn decide(
        &self,
        expected_state_revision: u64,
        command: WorkCompletionCommand,
    ) -> Result<WorkCompletionTransitionDecision, WorkCompletionTransitionError> {
        if expected_state_revision != self.state_revision {
            return Err(WorkCompletionTransitionError::StaleRevision {
                completion_id: self.id.clone(),
                expected: expected_state_revision,
                actual: self.state_revision,
                command,
            });
        }
        self.state.decide(&self.id, command)
    }
}
