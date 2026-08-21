//! Data carried by each TaskRun lifecycle state.

mod blocked;
mod cancelled;
mod completed;
mod design_updating;
mod failed;
mod implementing;
mod merging;
mod reviewing;
mod reworking;
mod stopping;

pub(crate) use blocked::*;
pub(crate) use cancelled::*;
pub(crate) use completed::*;
pub(crate) use design_updating::*;
pub(crate) use failed::*;
pub(crate) use implementing::*;
pub(crate) use merging::*;
pub(crate) use reviewing::*;
pub(crate) use reworking::*;
pub(crate) use stopping::*;

use serde::{Deserialize, Serialize};

use super::{TaskStopOrigin, TaskStopReason};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum TaskRunStateKind {
    DesignUpdating,
    Implementing,
    Merging,
    Reviewing,
    Reworking,
    Stopping,
    Blocked,
    Completed,
    Failed,
    Cancelled,
}

impl TaskRunStateKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::DesignUpdating => "designUpdating",
            Self::Implementing => "implementing",
            Self::Merging => "merging",
            Self::Reviewing => "reviewing",
            Self::Reworking => "reworking",
            Self::Stopping => "stopping",
            Self::Blocked => "blocked",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub(crate) const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    pub(crate) const fn allows_executor_spawn(self) -> bool {
        matches!(self, Self::Implementing | Self::Reworking)
    }

    pub(crate) const fn allows_planner_workspace_mutation(self) -> bool {
        matches!(self, Self::DesignUpdating | Self::Merging)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskGitFingerprint {
    pub(crate) workspace_root: String,
    pub(crate) git_common_dir: String,
    pub(crate) branch: String,
    pub(crate) head: String,
    pub(crate) base_commit: String,
    pub(crate) expected_head: String,
    pub(crate) operation: String,
    pub(crate) index_diff_hash: String,
    pub(crate) working_tree_diff_hash: String,
    pub(crate) untracked_content_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FinalizedDesign {
    pub(crate) head: String,
    pub(crate) commit: Option<String>,
    pub(crate) summary: String,
    pub(crate) fingerprint: TaskGitFingerprint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DesignWorkspaceObservation {
    pub(crate) sequence: u64,
    pub(crate) turn_id: Option<String>,
    pub(crate) tool_call_id: Option<String>,
    pub(crate) fingerprint: TaskGitFingerprint,
}

impl DesignWorkspaceObservation {
    pub(crate) fn baseline(fingerprint: TaskGitFingerprint) -> Self {
        Self {
            sequence: 0,
            turn_id: None,
            tool_call_id: None,
            fingerprint,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", content = "data", rename_all = "camelCase")]
pub(crate) enum DesignProgress {
    Updating,
    Finalized(Box<FinalizedDesign>),
}

impl DesignProgress {
    pub(crate) fn from_finalized(design: FinalizedDesign) -> Self {
        Self::Finalized(Box::new(design))
    }

    pub(crate) fn finalized(&self) -> Option<&FinalizedDesign> {
        match self {
            Self::Updating => None,
            Self::Finalized(design) => Some(design),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskStopRequest {
    pub(crate) origin: TaskStopOrigin,
    pub(crate) reason: TaskStopReason,
    pub(crate) requested_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum ReviewTarget {
    Delivery {
        work_unit_id: String,
        completion_id: String,
        completion_revision: u32,
        reviewed_head: String,
    },
    Integration {
        reviewed_head: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum BlockedRecovery {
    RetryMerge,
    ResumeRework,
    ManualOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub(crate) enum TaskRunState {
    DesignUpdating(DesignUpdatingState),
    Implementing(ImplementingState),
    Merging(MergingState),
    Reviewing(ReviewingState),
    Reworking(ReworkingState),
    Stopping(StoppingState),
    Blocked(BlockedState),
    Completed(CompletedState),
    Failed(FailedState),
    Cancelled(CancelledState),
}

impl TaskRunState {
    pub(crate) fn new(baseline: TaskGitFingerprint) -> Self {
        Self::DesignUpdating(DesignUpdatingState::new(baseline))
    }

    pub(crate) const fn kind(&self) -> TaskRunStateKind {
        match self {
            Self::DesignUpdating(_) => TaskRunStateKind::DesignUpdating,
            Self::Implementing(_) => TaskRunStateKind::Implementing,
            Self::Merging(_) => TaskRunStateKind::Merging,
            Self::Reviewing(_) => TaskRunStateKind::Reviewing,
            Self::Reworking(_) => TaskRunStateKind::Reworking,
            Self::Stopping(_) => TaskRunStateKind::Stopping,
            Self::Blocked(_) => TaskRunStateKind::Blocked,
            Self::Completed(_) => TaskRunStateKind::Completed,
            Self::Failed(_) => TaskRunStateKind::Failed,
            Self::Cancelled(_) => TaskRunStateKind::Cancelled,
        }
    }

    pub(crate) fn design(&self) -> &DesignProgress {
        match self {
            Self::DesignUpdating(state) => state.design(),
            Self::Implementing(state) => state.design(),
            Self::Merging(state) => state.design(),
            Self::Reviewing(state) => state.design(),
            Self::Reworking(state) => state.design(),
            Self::Stopping(state) => state.design(),
            Self::Blocked(state) => state.design(),
            Self::Completed(state) => state.design(),
            Self::Failed(state) => state.design(),
            Self::Cancelled(state) => state.design(),
        }
    }

    pub(crate) const fn generation(&self) -> u64 {
        match self {
            Self::DesignUpdating(state) => state.generation(),
            Self::Implementing(state) => state.generation(),
            Self::Merging(state) => state.generation(),
            Self::Reviewing(state) => state.generation(),
            Self::Reworking(state) => state.generation(),
            Self::Stopping(state) => state.generation(),
            Self::Blocked(state) => state.generation(),
            Self::Completed(state) => state.generation(),
            Self::Failed(state) => state.generation(),
            Self::Cancelled(state) => state.generation(),
        }
    }

    pub(crate) fn status_message(&self) -> Option<&str> {
        match self {
            Self::DesignUpdating(_) | Self::Implementing(_) | Self::Completed(_) => None,
            Self::Merging(state) => state.status_message(),
            Self::Reviewing(state) => state.status_message(),
            Self::Reworking(state) => Some(state.status_message()),
            Self::Stopping(state) => Some(state.status_message()),
            Self::Blocked(state) => Some(state.message()),
            Self::Failed(state) => Some(state.message()),
            Self::Cancelled(state) => Some(state.message()),
        }
    }

    pub(crate) fn stop_request(&self) -> Option<&TaskStopRequest> {
        match self {
            Self::Stopping(state) => Some(state.request()),
            Self::Cancelled(state) => state.request(),
            _ => None,
        }
    }

    pub(crate) fn terminal_failure_id(&self) -> Option<&str> {
        match self {
            Self::Failed(state) => state.failure_id(),
            _ => None,
        }
    }

    pub(crate) fn latest_design_observation(&self) -> Option<&DesignWorkspaceObservation> {
        match self {
            Self::DesignUpdating(state) => Some(state.latest_observation()),
            _ => None,
        }
    }
}
