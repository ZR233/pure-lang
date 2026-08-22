use serde::{Deserialize, Serialize};

/// 排队等待执行的 Turn。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QueuedTurnState {
    queued_at: i64,
}

impl QueuedTurnState {
    pub fn new(queued_at: i64) -> Self {
        Self { queued_at }
    }

    pub fn queued_at(&self) -> i64 {
        self.queued_at
    }
}
