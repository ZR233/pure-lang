use serde::{Deserialize, Serialize};
// ── DTO types ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStudioTurnDto {
    pub turn_id: String,
    pub session_id: String,
    pub status: String,
    pub reason: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStudioMessageDto {
    pub message_id: String,
    pub session_id: String,
    pub turn_id: String,
    pub role: String,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub completed_at: Option<i64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStudioPartDto {
    pub part_id: String,
    pub message_id: String,
    pub session_id: String,
    pub turn_id: String,
    pub part_type: String,
    pub order: u64,
    pub revision: u64,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub completed_at: Option<i64>,
    pub error: Option<String>,
    pub text_channel: Option<String>,
    pub activity_group_id: Option<String>,
    pub text: String,
    pub tool: Option<BridgeStudioToolPartDto>,
    pub agent: Option<BridgeStudioAgentPartDto>,
    pub plan: Option<BridgeStudioPlanPartDto>,
    pub synthetic: bool,
    pub ignored: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStudioToolPartDto {
    pub tool_call_id: String,
    pub call_id: Option<String>,
    pub provider_item_id: Option<String>,
    pub name: String,
    pub arguments: String,
    pub result: Option<String>,
    pub output_artifacts_json: String,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub working_directory: Option<String>,
    pub denial_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStudioAgentPartDto {
    pub id: String,
    pub path: String,
    pub parent_path: Option<String>,
    pub role: String,
    pub task: String,
    pub status: String,
    pub summary: Option<String>,
    pub depth: u32,
    pub error: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStudioPlanPartDto {
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStudioPartDeltaDto {
    pub part_id: String,
    pub revision: u64,
    pub field: String,
    pub delta: String,
    pub chunk_index: Option<u32>,
}
