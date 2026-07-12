use serde::{Deserialize, Serialize};
// ── Runtime types ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSnapshot {
    pub status: BridgeRuntimeStatus,
    pub active_turns: Vec<BridgeActiveTurn>,
    pub updated_at: i64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeRuntimeStatus {
    Uninitialized,
    Initializing,
    Ready,
    ShuttingDown,
    Stopped,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeActiveTurn {
    pub session_id: String,
    pub turn_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeSessionRuntimeDto {
    pub session_id: String,
    pub model: String,
    pub context_window: Option<u64>,
    pub latest_context_tokens: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cached_prompt_tokens: u64,
    pub total_tokens: u64,
    pub estimated_costs: Vec<BridgeRuntimeCostAmountDto>,
    pub has_unpriced_usage: bool,
    pub active_skills: Vec<String>,
    pub active_mcp_servers: Vec<String>,
    pub active_lsp_servers: Vec<String>,
    pub task: Option<BridgeTaskRuntimeDto>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeTaskRuntimeDto {
    pub run_id: String,
    pub phase: String,
    pub branch: String,
    pub expected_head: String,
    pub status_message: Option<String>,
    pub work_units: Vec<BridgeTaskWorkUnitDto>,
    pub agents: Vec<BridgeTaskAgentDto>,
    pub merges: Vec<BridgeTaskMergeDto>,
    pub reviews: Vec<BridgeTaskReviewDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeTaskWorkUnitDto {
    pub id: String,
    pub title: String,
    pub status: String,
    pub worktree_path: String,
    pub branch: String,
    pub agent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeTaskAgentDto {
    pub agent_id: String,
    pub role: String,
    pub status: String,
    pub initiated_by: String,
    pub requested_by_call_id: String,
    pub summary: Option<String>,
    pub error: Option<String>,
    pub head_commit: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeTaskMergeDto {
    pub id: String,
    pub agent_id: String,
    pub status: String,
    pub merge_commit: Option<String>,
    pub conflict_files: Vec<String>,
    pub resolution_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeTaskReviewDto {
    pub round: u32,
    pub head_commit: String,
    pub verdict: String,
    pub reviewer_agent_id: Option<String>,
    pub summary: Option<String>,
    pub design_references: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeRuntimeCostAmountDto {
    pub currency: String,
    pub amount: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeSkillActivationDto {
    pub name: String,
    pub source: String,
    pub path: String,
    pub turn_id: String,
    pub tool_call_id: String,
    pub activated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgePlanLifecycleDto {
    pub plan_id: String,
    pub state: String,
    pub turn_id: Option<String>,
    pub reason: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeMcpHealthDto {
    pub active_mcp_servers: Vec<String>,
    pub mcp_servers: Vec<BridgeMcpServerDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeMcpServerDto {
    pub id: String,
    pub enabled: bool,
    pub transport: String,
    pub command: Option<String>,
    pub url: Option<String>,
    pub endpoint: String,
    pub source_kind: String,
    pub status_kind: String,
    pub mutation_policy: String,
    pub availability_kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeLspHealthDto {
    pub active_lsp_servers: Vec<String>,
}
