use serde::{Deserialize, Serialize};

use crate::TurnCompletion;

/// 正常完成的 Turn 结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CompletedTurnOutcome {
    completion: TurnCompletion,
}

impl CompletedTurnOutcome {
    pub fn new(completion: TurnCompletion) -> Self {
        Self { completion }
    }

    pub fn completion(&self) -> TurnCompletion {
        self.completion
    }
}
