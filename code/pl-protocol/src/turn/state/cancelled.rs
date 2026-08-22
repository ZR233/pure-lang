use serde::{Deserialize, Serialize};

use crate::TurnCancellationCause;

/// 被显式取消的 Turn。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CancelledTurnState {
    started_at: Option<i64>,
    requested_at: i64,
    completed_at: i64,
    cause: TurnCancellationCause,
}

impl CancelledTurnState {
    pub fn new(
        started_at: Option<i64>,
        requested_at: i64,
        completed_at: i64,
        cause: TurnCancellationCause,
    ) -> Self {
        Self {
            started_at,
            requested_at,
            completed_at,
            cause,
        }
    }

    pub fn started_at(&self) -> Option<i64> {
        self.started_at
    }

    pub fn requested_at(&self) -> i64 {
        self.requested_at
    }

    pub fn completed_at(&self) -> i64 {
        self.completed_at
    }

    pub fn cause(&self) -> &TurnCancellationCause {
        &self.cause
    }
}
