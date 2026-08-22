use pl_protocol::TurnState;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TraceTurnPart {
    state: TurnState,
}

impl TraceTurnPart {
    pub fn new(state: TurnState) -> Self {
        Self { state }
    }

    pub fn state(&self) -> &TurnState {
        &self.state
    }

    pub(super) fn transition(&self, next_state: TurnState) -> Result<Self, &'static str> {
        use TurnState::{BudgetLimited, Cancelled, Completed, Failed, Queued, Running};

        let valid = match (&self.state, &next_state) {
            (Queued(_), Running(_))
            | (Queued(_), Cancelled(_))
            | (Queued(_), Failed(_))
            | (Running(_), Running(_))
            | (Running(_), Completed(_))
            | (Running(_), Cancelled(_))
            | (Running(_), Failed(_))
            | (Running(_), BudgetLimited(_)) => true,
            (current, next) if current == next => true,
            _ => false,
        };
        if !valid {
            return Err("illegal turn trace lifecycle transition");
        }
        Ok(Self { state: next_state })
    }
}
