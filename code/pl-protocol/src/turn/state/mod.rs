//! Turn lifecycle state payloads.

mod budget_limited;
mod cancelled;
mod completed;
mod failed;
mod queued;
mod running;

pub use budget_limited::BudgetLimitedTurnState;
pub use cancelled::CancelledTurnState;
pub use completed::CompletedTurnState;
pub use failed::FailedTurnState;
pub use queued::QueuedTurnState;
pub use running::RunningTurnState;

use serde::{Deserialize, Serialize};

use crate::{TurnFailure, TurnPhase};

/// Turn 的唯一 canonical 生命周期状态。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum TurnState {
    Queued(QueuedTurnState),
    Running(RunningTurnState),
    Completed(CompletedTurnState),
    Cancelled(CancelledTurnState),
    Failed(FailedTurnState),
    BudgetLimited(BudgetLimitedTurnState),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TurnStateKind {
    Queued,
    Running,
    Completed,
    Cancelled,
    Failed,
    BudgetLimited,
}

impl TurnState {
    pub(super) fn kind(&self) -> TurnStateKind {
        match self {
            Self::Queued(_) => TurnStateKind::Queued,
            Self::Running(_) => TurnStateKind::Running,
            Self::Completed(_) => TurnStateKind::Completed,
            Self::Cancelled(_) => TurnStateKind::Cancelled,
            Self::Failed(_) => TurnStateKind::Failed,
            Self::BudgetLimited(_) => TurnStateKind::BudgetLimited,
        }
    }

    pub fn started_at(&self) -> Option<i64> {
        match self {
            Self::Queued(_) => None,
            Self::Running(state) => Some(state.started_at()),
            Self::Completed(state) => state.started_at(),
            Self::Cancelled(state) => state.started_at(),
            Self::Failed(state) => state.started_at(),
            Self::BudgetLimited(state) => state.started_at(),
        }
    }

    pub fn completed_at(&self) -> Option<i64> {
        match self {
            Self::Queued(_) | Self::Running(_) => None,
            Self::Completed(state) => Some(state.completed_at()),
            Self::Cancelled(state) => Some(state.completed_at()),
            Self::Failed(state) => Some(state.completed_at()),
            Self::BudgetLimited(state) => Some(state.completed_at()),
        }
    }

    pub fn phase(&self) -> Option<TurnPhase> {
        match self {
            Self::Running(state) => Some(state.phase()),
            Self::Queued(_)
            | Self::Completed(_)
            | Self::Cancelled(_)
            | Self::Failed(_)
            | Self::BudgetLimited(_) => None,
        }
    }

    pub fn failure(&self) -> Option<&TurnFailure> {
        match self {
            Self::Failed(state) => Some(state.failure()),
            Self::Queued(_)
            | Self::Running(_)
            | Self::Completed(_)
            | Self::Cancelled(_)
            | Self::BudgetLimited(_) => None,
        }
    }

    pub fn is_terminal(&self) -> bool {
        match self {
            Self::Queued(_) | Self::Running(_) => false,
            Self::Completed(_) | Self::Cancelled(_) | Self::Failed(_) | Self::BudgetLimited(_) => {
                true
            }
        }
    }
}
