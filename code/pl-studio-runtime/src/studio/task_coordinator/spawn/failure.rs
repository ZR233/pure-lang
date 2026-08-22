use serde::{Deserialize, Serialize};

use crate::agent::worktree::{WorktreeError, WorktreeFailureCause};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum TaskSpawnFailureCode {
    #[serde(rename = "allocation_failed")]
    Allocation,
    #[serde(rename = "worktree_create_failed")]
    WorktreeCreate,
    #[serde(rename = "child_thread_create_failed")]
    ChildThreadCreate,
    #[serde(rename = "agent_registration_failed")]
    AgentRegistration,
    #[serde(rename = "activation_failed")]
    Activation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum TaskSpawnFailurePhase {
    Allocation,
    WorktreeCreate,
    ChildThreadCreate,
    AgentRegistration,
    Activation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum TaskSpawnCompensationState {
    NotCreated,
    MarkedFailed,
    Removed,
    Faulted,
    CleanupFailed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskSpawnResource {
    pub(crate) repo_root: String,
    pub(crate) path: String,
    pub(crate) branch: String,
    pub(crate) base_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskSpawnCompensation {
    pub(crate) allocation: TaskSpawnCompensationState,
    pub(crate) worktree: TaskSpawnCompensationState,
    pub(crate) child_thread: TaskSpawnCompensationState,
}

pub(crate) struct OperationalTaskSpawnFailure {
    pub(crate) code: TaskSpawnFailureCode,
    pub(crate) phase: TaskSpawnFailurePhase,
    pub(crate) message: String,
    pub(crate) task_run_id: Option<String>,
    pub(crate) work_unit_id: Option<String>,
    pub(crate) agent_id: String,
    pub(crate) resource: Option<TaskSpawnResource>,
    pub(crate) compensation: TaskSpawnCompensation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskSpawnFailure {
    pub(crate) status: String,
    pub(crate) code: TaskSpawnFailureCode,
    pub(crate) phase: TaskSpawnFailurePhase,
    pub(crate) recoverable: bool,
    pub(crate) message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) task_run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) work_unit_id: Option<String>,
    pub(crate) agent_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) resource: Option<TaskSpawnResource>,
    pub(crate) cause: WorktreeFailureCause,
    pub(crate) compensation: TaskSpawnCompensation,
    pub(crate) next_action: String,
}

impl TaskSpawnFailure {
    pub(crate) fn worktree(
        task_run_id: String,
        work_unit_id: String,
        agent_id: String,
        resource: TaskSpawnResource,
        error: &WorktreeError,
    ) -> Self {
        let worktree = if error.cleanup_failed() {
            TaskSpawnCompensationState::CleanupFailed
        } else if error.cleanup_succeeded() {
            TaskSpawnCompensationState::Removed
        } else {
            TaskSpawnCompensationState::NotCreated
        };
        Self {
            status: "failed".to_string(),
            code: TaskSpawnFailureCode::WorktreeCreate,
            phase: TaskSpawnFailurePhase::WorktreeCreate,
            recoverable: !error.cleanup_failed(),
            message: error.to_string(),
            task_run_id: Some(task_run_id),
            work_unit_id: Some(work_unit_id),
            agent_id,
            resource: Some(resource),
            cause: error.cause(),
            compensation: TaskSpawnCompensation {
                allocation: if error.cleanup_failed() {
                    TaskSpawnCompensationState::Faulted
                } else {
                    TaskSpawnCompensationState::MarkedFailed
                },
                worktree,
                child_thread: TaskSpawnCompensationState::NotCreated,
            },
            next_action: if error.cleanup_failed() {
                "recover_worktree_resources".to_string()
            } else {
                "retry_task_spawn_executor".to_string()
            },
        }
    }

    pub(crate) fn operational(input: OperationalTaskSpawnFailure) -> Self {
        let OperationalTaskSpawnFailure {
            code,
            phase,
            message,
            task_run_id,
            work_unit_id,
            agent_id,
            resource,
            compensation,
        } = input;
        let cleanup_failed = compensation.worktree == TaskSpawnCompensationState::CleanupFailed;
        Self {
            status: "failed".to_string(),
            code,
            phase,
            recoverable: !cleanup_failed,
            cause: WorktreeFailureCause {
                kind: crate::agent::worktree::WorktreeFailureCauseKind::Io,
                message: message.clone(),
                args: None,
                exit_code: None,
                stderr: None,
            },
            message,
            task_run_id,
            work_unit_id,
            agent_id,
            resource,
            compensation,
            next_action: if cleanup_failed {
                "recover_worktree_resources".to_string()
            } else {
                "retry_task_spawn_executor".to_string()
            },
        }
    }

    pub(crate) fn needs_attention(&self) -> bool {
        self.compensation.worktree == TaskSpawnCompensationState::CleanupFailed
            || self.compensation.allocation == TaskSpawnCompensationState::Faulted
            || self.compensation.child_thread == TaskSpawnCompensationState::Faulted
    }

    pub(crate) fn allocation(
        task_run_id: Option<String>,
        agent_id: String,
        message: String,
    ) -> Self {
        Self::operational(OperationalTaskSpawnFailure {
            code: TaskSpawnFailureCode::Allocation,
            phase: TaskSpawnFailurePhase::Allocation,
            message,
            task_run_id,
            work_unit_id: None,
            agent_id,
            resource: None,
            compensation: TaskSpawnCompensation {
                allocation: TaskSpawnCompensationState::NotCreated,
                worktree: TaskSpawnCompensationState::NotCreated,
                child_thread: TaskSpawnCompensationState::NotCreated,
            },
        })
    }
}
