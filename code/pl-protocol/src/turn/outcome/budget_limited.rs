use serde::{Deserialize, Serialize};

use crate::{BudgetLimitSnapshot, TurnRolloverOutcome};

/// 预算耗尽的 Turn 结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BudgetLimitedTurnOutcome {
    limit: BudgetLimitSnapshot,
    rollover: TurnRolloverOutcome,
}

impl BudgetLimitedTurnOutcome {
    pub fn new(limit: BudgetLimitSnapshot, rollover: TurnRolloverOutcome) -> Self {
        Self { limit, rollover }
    }

    pub fn limit(&self) -> &BudgetLimitSnapshot {
        &self.limit
    }

    pub fn rollover(&self) -> &TurnRolloverOutcome {
        &self.rollover
    }

    pub fn replace_rollover(&mut self, rollover: TurnRolloverOutcome) {
        self.rollover = rollover;
    }
}
