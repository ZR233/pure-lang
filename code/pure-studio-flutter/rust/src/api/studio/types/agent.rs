use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeAgentSnapshotDto {
    pub id: String,
    pub session_id: String,
    pub path: String,
    pub parent_path: Option<String>,
    pub role: String,
    pub task: String,
    pub status: String,
    pub summary: Option<String>,
    pub depth: u32,
    pub error: Option<String>,
    pub reason: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeAgentTimelineEventDto {
    pub event_id: String,
    pub session_id: String,
    pub sequence: u64,
    pub created_at: i64,
    pub payload: BridgeAgentTimelinePayloadDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeAgentTimelinePayloadDto {
    SpawnBegin {
        call_id: String,
        sender_path: String,
        task_name: String,
        prompt: String,
        role: String,
        model: Option<String>,
        reasoning_effort: Option<String>,
    },
    SpawnEnd {
        call_id: String,
        sender_path: String,
        agent_id: Option<String>,
        path: Option<String>,
        role: Option<String>,
        status: String,
        prompt: String,
        error: Option<String>,
    },
    InteractionBegin {
        call_id: String,
        sender_path: String,
        receiver_path: String,
        prompt: String,
    },
    InteractionEnd {
        call_id: String,
        sender_path: String,
        receiver_path: String,
        status: String,
        prompt: String,
        error: Option<String>,
    },
    WaitingBegin {
        call_id: String,
        sender_path: String,
    },
    WaitingEnd {
        call_id: String,
        sender_path: String,
        timed_out: bool,
    },
    CloseBegin {
        call_id: String,
        sender_path: String,
        receiver_path: String,
    },
    CloseEnd {
        call_id: String,
        sender_path: String,
        receiver_path: String,
        status: String,
        error: Option<String>,
    },
}
