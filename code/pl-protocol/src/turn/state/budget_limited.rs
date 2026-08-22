use serde::{Deserialize, Serialize};

use crate::{BudgetLimitSnapshot, TurnRolloverOutcome};

/// 达到预算上限后结束的 Turn。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BudgetLimitedTurnState {
    started_at: Option<i64>,
    completed_at: i64,
    limit: BudgetLimitSnapshot,
    rollover: TurnRolloverOutcome,
}

impl BudgetLimitedTurnState {
    pub fn new(
        started_at: Option<i64>,
        completed_at: i64,
        limit: BudgetLimitSnapshot,
        rollover: TurnRolloverOutcome,
    ) -> Self {
        Self {
            started_at,
            completed_at,
            limit,
            rollover,
        }
    }

    pub fn started_at(&self) -> Option<i64> {
        self.started_at
    }

    pub fn completed_at(&self) -> i64 {
        self.completed_at
    }

    pub fn limit(&self) -> &BudgetLimitSnapshot {
        &self.limit
    }

    pub fn rollover(&self) -> &TurnRolloverOutcome {
        &self.rollover
    }
}
