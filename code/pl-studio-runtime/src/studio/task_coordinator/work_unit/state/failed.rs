use serde::{Deserialize, Serialize};

use crate::studio::task_coordinator::{TaskSpawnFailure, TaskWorktreeDisposition};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub(crate) enum WorkUnitFailure {
    Spawn(Box<TaskSpawnFailure>),
    Execution {
        operation_id: String,
        detail: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct FailedWorkUnit {
    pub(super) failure: WorkUnitFailure,
    pub(super) worktree_disposition: TaskWorktreeDisposition,
}

impl FailedWorkUnit {
    pub(crate) const fn failure(&self) -> &WorkUnitFailure {
        &self.failure
    }

    pub(crate) const fn worktree_disposition(&self) -> TaskWorktreeDisposition {
        self.worktree_disposition
    }
}
