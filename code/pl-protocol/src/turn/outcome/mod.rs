//! Turn terminal outcome shared by the engine, runtime and protocol projector.

mod budget_limited;
mod cancelled;
mod completed;
mod failed;

pub use budget_limited::BudgetLimitedTurnOutcome;
pub use cancelled::CancelledTurnOutcome;
pub use completed::CompletedTurnOutcome;
pub use failed::FailedTurnOutcome;

use serde::{Deserialize, Serialize};

use crate::{
    BudgetLimitSnapshot, TurnCancellationCause, TurnCompletion, TurnFailure, TurnRolloverOutcome,
};

/// Turn 的强类型终止结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum TurnOutcome {
    Completed(CompletedTurnOutcome),
    Cancelled(CancelledTurnOutcome),
    Failed(FailedTurnOutcome),
    BudgetLimited(BudgetLimitedTurnOutcome),
}

impl TurnOutcome {
    pub fn completed(completion: TurnCompletion) -> Self {
        Self::Completed(CompletedTurnOutcome::new(completion))
    }

    pub fn cancelled(cause: TurnCancellationCause) -> Self {
        Self::Cancelled(CancelledTurnOutcome::new(cause))
    }

    pub fn failed(failure: TurnFailure) -> Self {
        Self::Failed(FailedTurnOutcome::new(failure))
    }

    pub fn budget_limited(limit: BudgetLimitSnapshot, rollover: TurnRolloverOutcome) -> Self {
        Self::BudgetLimited(BudgetLimitedTurnOutcome::new(limit, rollover))
    }

    pub fn failure(&self) -> Option<&TurnFailure> {
        match self {
            Self::Failed(outcome) => Some(outcome.failure()),
            Self::Completed(_) | Self::Cancelled(_) | Self::BudgetLimited(_) => None,
        }
    }

    pub fn is_completed(&self) -> bool {
        matches!(self, Self::Completed(_))
    }

    pub fn is_interaction_boundary(&self) -> bool {
        matches!(
            self,
            Self::Completed(outcome)
                if outcome.completion() == TurnCompletion::InteractionRequested
        )
    }
}
