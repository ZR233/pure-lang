use super::agent::{BridgeAgentSnapshotDto, BridgeAgentTimelineEventDto};
use super::interaction::BridgeInteractionChangedDto;
use super::message::{
    BridgeStudioMessageDto, BridgeStudioPartDeltaDto, BridgeStudioPartDto, BridgeStudioTurnDto,
};
use super::response::SessionDto;
use super::runtime::{
    BridgeLspHealthDto, BridgeMcpHealthDto, BridgePlanLifecycleDto, BridgeSessionRuntimeDto,
    BridgeSkillActivationDto,
};
use serde::{Deserialize, Serialize};
// ── Event types ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeEventEnvelope {
    pub event_id: String,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub sequence: u64,
    pub created_at: i64,
    pub payload: BridgeEventPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeEventPayload {
    TurnChanged {
        turn: BridgeStudioTurnDto,
    },
    MessageUpdated {
        message: BridgeStudioMessageDto,
    },
    MessageRemoved {
        message_id: String,
    },
    MessagePartUpdated {
        part: Box<BridgeStudioPartDto>,
    },
    MessagePartRemoved {
        message_id: String,
        part_id: String,
    },
    MessagePartDelta {
        delta: BridgeStudioPartDeltaDto,
    },
    InteractionChanged {
        event: BridgeInteractionChangedDto,
    },
    AgentChanged {
        agent: Box<BridgeAgentSnapshotDto>,
    },
    AgentTimelineChanged {
        event: BridgeAgentTimelineEventDto,
    },
    SessionRuntimeChanged {
        runtime: BridgeSessionRuntimeDto,
    },
    SkillActivated {
        activation: BridgeSkillActivationDto,
    },
    PlanLifecycleChanged {
        event: BridgePlanLifecycleDto,
    },
    SessionListChanged {
        project_id: String,
        sessions: Vec<SessionDto>,
    },
    McpHealthChanged {
        health: BridgeMcpHealthDto,
    },
    LspHealthChanged {
        health: BridgeLspHealthDto,
    },
    Stale {
        lagged_events: u64,
    },
}

// ── BridgeEventEnvelope helpers ──

impl BridgeEventEnvelope {
    pub fn stale(session_id: Option<String>, lagged_events: u64) -> Self {
        Self {
            event_id: {
                use std::time::{SystemTime, UNIX_EPOCH};
                let nanos = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos();
                let suffix = format!("{nanos:x}");
                format!("bridge-stale-{suffix}")
            },
            session_id,
            turn_id: None,
            sequence: 0,
            created_at: {
                use std::time::{SystemTime, UNIX_EPOCH};
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64
            },
            payload: BridgeEventPayload::Stale { lagged_events },
        }
    }
}
