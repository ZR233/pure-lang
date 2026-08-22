use serde::{Deserialize, Serialize};

use crate::TurnCancellationCause;

/// 取消的 Turn 结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CancelledTurnOutcome {
    cause: TurnCancellationCause,
}

impl CancelledTurnOutcome {
    pub fn new(cause: TurnCancellationCause) -> Self {
        Self { cause }
    }

    pub fn cause(&self) -> &TurnCancellationCause {
        &self.cause
    }
}
