use serde::{Deserialize, Serialize};

use crate::agent_runtime::{AgentId, ThreadId, TurnId};

use super::{lifecycle::*, mailbox::*, snapshot::*};

/// 提交后广播的 framework runtime 事件。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRuntimeEvent {
    pub agent_id: AgentId,
    pub sequence: u64,
    pub created_at: i64,
    pub kind: AgentRuntimeEventKind,
}

/// runtime 事件的结构化类型。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum AgentRuntimeEventKind {
    Registered {
        snapshot: AgentSnapshot,
    },
    StateChanged {
        snapshot: AgentSnapshot,
    },
    TurnQueued {
        input: DurableMailboxEnvelope,
        snapshot: AgentSnapshot,
    },
    TurnStarted {
        turn_id: TurnId,
        thread_id: ThreadId,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        claimed_inputs: Vec<DurableMailboxEnvelope>,
        snapshot: AgentSnapshot,
    },
    ThreadOpened {
        thread_id: ThreadId,
        snapshot: AgentSnapshot,
    },
    TurnActivityChanged {
        turn_id: TurnId,
        thread_id: ThreadId,
        kind: ActiveKind,
        snapshot: AgentSnapshot,
    },
    TurnFinished {
        outcome: AgentTurnOutcome,
        snapshot: AgentSnapshot,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        finalized_with_tool: Option<String>,
    },
    RecoveryCancelledTurn {
        outcome: AgentTurnOutcome,
        snapshot: AgentSnapshot,
    },
    Faulted {
        reason: String,
        snapshot: AgentSnapshot,
    },
}
