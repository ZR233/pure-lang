use serde::{Deserialize, Serialize};

use crate::{
    InteractionChangedEvent, PlanLifecycleEvent, SkillActivation, TimelineItem,
    TimelineItemDeltaEvent,
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
#[serde(rename_all = "camelCase", tag = "type")]
pub enum StudioEventKind {
    TurnChanged { turn: StudioTurn },
    TimelineChanged { change: Box<StudioTimelineChange> },
    InteractionChanged { event: Box<InteractionChangedEvent> },
    AgentChanged { agent: StudioAgentSnapshot },
    AgentTimelineChanged { event: StudioAgentTimelineEvent },
    SessionRuntimeChanged { runtime: StudioSessionRuntime },
    SkillActivated { activation: SkillActivation },
    PlanLifecycleChanged { event: PlanLifecycleEvent },
    SessionHandoffChanged { handoff: StudioSessionHandoff },
    SessionListChanged { sessions: Vec<StudioSessionSummary> },
    McpHealthChanged { health: StudioMcpHealth },
    LspHealthChanged { health: StudioLspHealth },
    Stale { lagged_events: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioTurn {
    pub turn_id: String,
    pub session_id: String,
    pub status: StudioTurnStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioTurnStatus {
    Queued,
    ContextLoading,
    WaitingForModel,
    Streaming,
    WaitingForInteraction,
    RunningTool,
    Persisting,
    Completed,
    Failed,
    Cancelled,
}

impl StudioTurnStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::ContextLoading => "contextLoading",
            Self::WaitingForModel => "waitingForModel",
            Self::Streaming => "streaming",
            Self::WaitingForInteraction => "waitingForInteraction",
            Self::RunningTool => "runningTool",
            Self::Persisting => "persisting",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum StudioTimelineChange {
    Started {
        item: TimelineItem,
    },
    Delta {
        event: TimelineItemDeltaEvent,
    },
    Completed {
        sequence: u64,
        item: TimelineItem,
    },
    Failed {
        sequence: u64,
        item: TimelineItem,
        error: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioAgentSnapshot {
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioAgentTimelineEvent {
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioSessionRuntime {
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioSessionHandoff {
    pub origin_session_id: String,
    pub target_session_id: String,
    pub kind: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_id: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioSessionSummary {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub mode: String,
    pub updated_at: i64,
    pub visibility: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioMcpHealth {
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioLspHealth {
    pub payload: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn studio_turn_status_is_camel_case() {
        assert_eq!(
            serde_json::to_value(StudioTurnStatus::WaitingForModel).unwrap(),
            serde_json::json!("waitingForModel")
        );
    }
}
