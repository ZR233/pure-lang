use serde::{Deserialize, Serialize};

use crate::studio::task_coordinator::TaskWorktreeDisposition;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CancelledWorkUnit {
    pub(super) operation_id: String,
    pub(super) reason: String,
    pub(super) worktree_disposition: TaskWorktreeDisposition,
}

impl CancelledWorkUnit {
    pub(crate) fn operation_id(&self) -> &str {
        &self.operation_id
    }

    pub(crate) fn reason(&self) -> &str {
        &self.reason
    }

    pub(crate) const fn worktree_disposition(&self) -> TaskWorktreeDisposition {
        self.worktree_disposition
    }
}
