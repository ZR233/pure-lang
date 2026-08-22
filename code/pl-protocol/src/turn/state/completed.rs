use serde::{Deserialize, Serialize};

use crate::TurnCompletion;

/// 正常结束的 Turn。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CompletedTurnState {
    started_at: Option<i64>,
    completed_at: i64,
    completion: TurnCompletion,
}

impl CompletedTurnState {
    pub fn new(started_at: Option<i64>, completed_at: i64, completion: TurnCompletion) -> Self {
        Self {
            started_at,
            completed_at,
            completion,
        }
    }

    pub fn started_at(&self) -> Option<i64> {
        self.started_at
    }

    pub fn completed_at(&self) -> i64 {
        self.completed_at
    }

    pub fn completion(&self) -> TurnCompletion {
        self.completion
    }
}
