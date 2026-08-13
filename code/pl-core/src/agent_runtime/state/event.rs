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
        snapshot: Box<AgentSnapshot>,
    },
    StateChanged {
        snapshot: Box<AgentSnapshot>,
    },
    TurnQueued {
        input: DurableMailboxEnvelope,
        snapshot: Box<AgentSnapshot>,
    },
    TurnStarted {
        turn_id: TurnId,
        thread_id: ThreadId,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        claimed_inputs: Vec<DurableMailboxEnvelope>,
        snapshot: Box<AgentSnapshot>,
    },
    ThreadOpened {
        thread_id: ThreadId,
        snapshot: Box<AgentSnapshot>,
    },
    TurnActivityChanged {
        turn_id: TurnId,
        thread_id: ThreadId,
        kind: ActiveKind,
        snapshot: Box<AgentSnapshot>,
    },
    TurnFinished {
        outcome: AgentTurnOutcome,
        snapshot: Box<AgentSnapshot>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        finalized_with_tool: Option<String>,
    },
    RecoveryCancelledTurn {
        outcome: AgentTurnOutcome,
        snapshot: Box<AgentSnapshot>,
    },
    Faulted {
        reason: String,
        snapshot: Box<AgentSnapshot>,
    },
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn boxed_snapshot_keeps_event_wire_flat_and_enum_compact() {
        // 装箱后的 enum 必须小于旧 TurnQueued 直接内嵌 snapshot 与 envelope 的尺寸和。
        assert!(
            std::mem::size_of::<AgentRuntimeEventKind>()
                < std::mem::size_of::<AgentSnapshot>()
                    + std::mem::size_of::<DurableMailboxEnvelope>()
        );

        let snapshot = AgentRegistration {
            identity: AgentIdentity {
                id: AgentId::new("agent-1").unwrap(),
                parent_id: None,
                role: crate::AgentRoleId::new("planner").unwrap(),
                depth: 0,
            },
            session: ThreadContextState::empty(),
            runtime_revision: 1,
            event_sequence: 2,
        }
        .into_durable_state()
        .snapshot;
        let value = serde_json::to_value(AgentRuntimeEventKind::Registered {
            snapshot: Box::new(snapshot.clone()),
        })
        .unwrap();

        assert_eq!(value["type"], "registered");
        assert_eq!(value["snapshot"]["identity"]["id"], "agent-1");
        assert_eq!(value["snapshot"]["revision"], json!(1));
        assert!(value.get("snapshot").is_some());
    }
}
