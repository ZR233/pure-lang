use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioTaskRuntime {
    pub run_id: String,
    pub phase: String,
    pub branch: String,
    pub expected_head: String,
    pub status_message: Option<String>,
    pub work_units: Vec<StudioTaskWorkUnitRuntime>,
    pub agents: Vec<StudioTaskAgentRuntime>,
    pub merges: Vec<StudioTaskMergeRuntime>,
    pub reviews: Vec<StudioTaskReviewRuntime>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioTaskWorkUnitRuntime {
    pub id: String,
    pub title: String,
    pub status: String,
    pub worktree_path: String,
    pub branch: String,
    pub agent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioTaskAgentRuntime {
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
pub struct StudioTaskMergeRuntime {
    pub id: String,
    pub agent_id: String,
    pub status: String,
    pub merge_commit: Option<String>,
    pub conflict_files: Vec<String>,
    pub resolution_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioTaskReviewRuntime {
    pub round: u32,
    pub head_commit: String,
    pub verdict: String,
    pub reviewer_agent_id: Option<String>,
    pub summary: Option<String>,
    pub design_references: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioSessionHandoff {
    pub origin_session_id: String,
    pub target_session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_session: Option<StudioSessionSummary>,
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
    pub created_at: i64,
    pub updated_at: i64,
    pub visibility: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    pub root_session_id: String,
    pub session_kind: String,
    pub owner_agent_id: String,
    pub owner_role: String,
    pub agent_status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_updated_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioMcpHealth {
    pub mcp_servers: Vec<StudioMcpServer>,
    pub active_mcp_servers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioLspHealth {
    pub lsp_servers: Vec<StudioLspServer>,
    pub active_lsp_servers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioKeyValue {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioMcpServer {
    pub id: String,
    pub enabled: bool,
    pub transport: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: Vec<StudioKeyValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bearer_token_env_var: Option<String>,
    #[serde(default)]
    pub headers: Vec<StudioKeyValue>,
    pub endpoint: String,
    pub source_kind: String,
    pub source_label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_detail: Option<String>,
    pub status_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_message: Option<String>,
    pub mutation_policy: String,
    pub availability_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub availability_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_checked_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_count: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioLspServer {
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub language_ids: Vec<String>,
    pub availability_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub availability_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_checked_at: Option<i64>,
    pub diagnostic_count: u64,
    pub activity_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity_percentage: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error_at: Option<i64>,
}
