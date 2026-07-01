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
    SubAgentActivity {
        call_id: String,
        agent_id: Option<String>,
        path: Option<String>,
        parent_path: Option<String>,
        kind: String,
        status: Option<String>,
        message: Option<String>,
        timed_out: bool,
        error: Option<String>,
    },
}
