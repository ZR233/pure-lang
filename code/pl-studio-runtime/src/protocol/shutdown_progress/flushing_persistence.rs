use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FlushingPersistenceProgress {
    pending_commits: u64,
}

impl FlushingPersistenceProgress {
    pub fn new(pending_commits: u64) -> Self {
        Self { pending_commits }
    }

    pub fn pending_commits(self) -> u64 {
        self.pending_commits
    }
}
