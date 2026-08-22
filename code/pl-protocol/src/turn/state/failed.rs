use serde::{Deserialize, Serialize};

use crate::TurnFailure;

/// 失败结束的 Turn。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FailedTurnState {
    started_at: Option<i64>,
    completed_at: i64,
    failure: TurnFailure,
}

impl FailedTurnState {
    pub fn new(started_at: Option<i64>, completed_at: i64, failure: TurnFailure) -> Self {
        Self {
            started_at,
            completed_at,
            failure,
        }
    }

    pub fn started_at(&self) -> Option<i64> {
        self.started_at
    }

    pub fn completed_at(&self) -> i64 {
        self.completed_at
    }

    pub fn failure(&self) -> &TurnFailure {
        &self.failure
    }
}
