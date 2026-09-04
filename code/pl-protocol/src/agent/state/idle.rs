use serde::{Deserialize, Serialize};

use crate::{BudgetLimitSnapshot, TurnId};

/// child Agent 因 Turn 预算耗尽而保持 idle 的持久化原因。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentBudgetPause {
    turn_id: TurnId,
    limit: BudgetLimitSnapshot,
    paused_at: i64,
}

impl AgentBudgetPause {
    pub fn new(turn_id: TurnId, limit: BudgetLimitSnapshot, paused_at: i64) -> Self {
        Self {
            turn_id,
            limit,
            paused_at,
        }
    }

    pub fn turn_id(&self) -> &TurnId {
        &self.turn_id
    }

    pub fn limit(&self) -> &BudgetLimitSnapshot {
        &self.limit
    }

    pub fn paused_at(&self) -> i64 {
        self.paused_at
    }
}

/// 没有 active 或 queued Turn 的 Agent。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IdleAgentState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    budget_pause: Option<AgentBudgetPause>,
}

impl IdleAgentState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn budget_paused(pause: AgentBudgetPause) -> Self {
        Self {
            budget_pause: Some(pause),
        }
    }

    pub fn budget_pause(&self) -> Option<&AgentBudgetPause> {
        self.budget_pause.as_ref()
    }
}
