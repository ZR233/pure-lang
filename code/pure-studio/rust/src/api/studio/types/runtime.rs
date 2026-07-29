use serde::{Deserialize, Serialize};
// ── Runtime types ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSnapshot {
    pub status: BridgeRuntimeStatus,
    pub active_turns: Vec<BridgeActiveTurn>,
    pub updated_at: i64,
    pub error: Option<String>,
    pub recovery_issues: Vec<BridgeStudioRecoveryIssueDto>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeRecoveryIssueScope {
    Application,
    Project,
    Session,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeRecoveryIssueCategory {
    ProcessLease,
    AgentState,
    Worktree,
    Repository,
    Merge,
    Conflict,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeRecoveryIssueAction {
    Retry,
    CleanupSession,
    RemoveProject,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStudioRecoveryIssueDto {
    pub id: String,
    pub scope: BridgeRecoveryIssueScope,
    pub category: BridgeRecoveryIssueCategory,
    pub available_actions: Vec<BridgeRecoveryIssueAction>,
    pub project_id: Option<String>,
    pub session_id: Option<String>,
    pub task_run_id: Option<String>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeRecoveryResourcePresence {
    Absent,
    Complete,
    Partial,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeRecoveryCleanupResourceDto {
    pub work_unit_id: String,
    pub path: String,
    pub branch: String,
    pub presence: BridgeRecoveryResourcePresence,
    pub registration_exists: bool,
    pub path_exists: bool,
    pub branch_exists: bool,
    pub branch_head: Option<String>,
    pub dirty: bool,
    pub ahead_by: u32,
    pub changed_file_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeRecoveryCleanupPreviewDto {
    pub issue_id: String,
    pub expected_revision: String,
    pub scope: BridgeRecoveryIssueScope,
    pub project_id: Option<String>,
    pub session_id: Option<String>,
    pub detail: String,
    pub resources: Vec<BridgeRecoveryCleanupResourceDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeTaskRuntimeDto {
    pub run_id: String,
    pub phase: String,
    pub branch: String,
    pub expected_head: String,
    pub status_message: Option<String>,
    pub stop_requested_origin: Option<String>,
    pub stop_requested_reason: Option<String>,
    pub task_generation: u64,
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
