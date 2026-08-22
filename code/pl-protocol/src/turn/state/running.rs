use serde::{Deserialize, Serialize};

use crate::TurnPhase;

/// 正在执行的 Turn。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunningTurnState {
    started_at: i64,
    phase: TurnPhase,
}

impl RunningTurnState {
    pub fn new(started_at: i64, phase: TurnPhase) -> Self {
        Self { started_at, phase }
    }

    pub fn started_at(&self) -> i64 {
        self.started_at
    }

    pub fn phase(&self) -> TurnPhase {
        self.phase
    }

    pub(crate) fn advance(self, phase: TurnPhase) -> Self {
        Self { phase, ..self }
    }
}
