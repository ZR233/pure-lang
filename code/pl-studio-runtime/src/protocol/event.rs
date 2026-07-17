use serde::{Deserialize, Serialize};

use pl_protocol::{InteractionChangedEvent, PlanLifecycleEvent, SkillActivation};

use super::{
    StudioAgentSnapshot, StudioAgentTimelineEvent, StudioLspHealth, StudioMcpHealth, StudioMessage,
    StudioPart, StudioPartDelta, StudioSessionHandoff, StudioSessionRuntime, StudioSessionSummary,
    StudioTurn,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioEventEnvelope {
    pub event_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    pub sequence: u64,
    pub created_at: i64,
    pub kind: StudioEventKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum StudioEventKind {
    TurnChanged {
        turn: StudioTurn,
    },
    MessageUpdated {
        message: Box<StudioMessage>,
    },
    MessageRemoved {
        message_id: String,
    },
    MessagePartUpdated {
        part: Box<StudioPart>,
    },
    MessagePartRemoved {
        message_id: String,
        part_id: String,
    },
    MessagePartDelta {
        delta: StudioPartDelta,
    },
    InteractionChanged {
        event: Box<InteractionChangedEvent>,
    },
    AgentChanged {
        agent: StudioAgentSnapshot,
    },
    AgentTimelineChanged {
        event: StudioAgentTimelineEvent,
    },
    SessionRuntimeChanged {
        runtime: StudioSessionRuntime,
    },
    SkillActivated {
        activation: SkillActivation,
    },
    PlanLifecycleChanged {
        event: PlanLifecycleEvent,
    },
    SessionHandoffChanged {
        handoff: StudioSessionHandoff,
    },
    SessionListChanged {
        project_id: String,
        sessions: Vec<StudioSessionSummary>,
    },
    McpHealthChanged {
        health: StudioMcpHealth,
    },
    LspHealthChanged {
        health: StudioLspHealth,
    },
    Stale {
        lagged_events: u64,
    },
}
