use serde::{Deserialize, Serialize};

use crate::TurnFailure;

/// 失败的 Turn 结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FailedTurnOutcome {
    failure: TurnFailure,
}

impl FailedTurnOutcome {
    pub fn new(failure: TurnFailure) -> Self {
        Self { failure }
    }

    pub fn failure(&self) -> &TurnFailure {
        &self.failure
    }
}
