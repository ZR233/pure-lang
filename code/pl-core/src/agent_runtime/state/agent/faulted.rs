use serde::{Deserialize, Serialize};

use crate::agent_runtime::TurnId;
use pl_protocol::StateError;

/// Faulted 状态的稳定恢复分类；旧快照缺少该字段时保持未知并拒绝自动恢复。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentFaultClassification {
    RecoverableRuntime,
    RecoverableProtocol,
    AggregateCorruption,
    #[default]
    LegacyUnknown,
}

impl AgentFaultClassification {
    pub const fn is_recoverable(self) -> bool {
        matches!(self, Self::RecoverableRuntime | Self::RecoverableProtocol)
    }
}

/// 因内部错误停止工作的 Agent 终态。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FaultedAgentState {
    error: StateError,
    turn_id: Option<TurnId>,
    #[serde(default)]
    classification: AgentFaultClassification,
}

impl FaultedAgentState {
    pub fn new(error: StateError, turn_id: Option<TurnId>) -> Self {
        Self {
            error,
            turn_id,
            classification: AgentFaultClassification::LegacyUnknown,
        }
    }

    pub fn classified(
        error: StateError,
        turn_id: Option<TurnId>,
        classification: AgentFaultClassification,
    ) -> Self {
        Self {
            error,
            turn_id,
            classification,
        }
    }

    pub fn error(&self) -> &StateError {
        &self.error
    }

    pub fn turn_id(&self) -> Option<&TurnId> {
        self.turn_id.as_ref()
    }

    pub const fn classification(&self) -> AgentFaultClassification {
        self.classification
    }
}
