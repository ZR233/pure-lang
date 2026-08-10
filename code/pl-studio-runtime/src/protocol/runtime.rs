use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioTaskRuntime {
    pub run_id: String,
    pub phase: String,
    pub branch: String,
    pub expected_head: String,
    pub status_message: Option<String>,
    pub stop_requested_origin: Option<String>,
    pub stop_requested_reason: Option<String>,
    pub task_generation: u64,
    pub failures: Vec<StudioTaskFailureRuntime>,
    pub terminal_failure: Option<StudioTaskFailureRuntime>,
    pub work_units: Vec<StudioTaskWorkUnitRuntime>,
    pub completions: Vec<StudioTaskCompletionRuntime>,
    pub merges: Vec<StudioTaskMergeRuntime>,
    pub reviews: Vec<StudioTaskReviewRuntime>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioTaskFailureRuntime {
    pub id: String,
    pub source_thread_id: String,
    pub source_turn_id: String,
    pub source_agent_id: String,
    pub source_role: String,
    pub work_unit_id: Option<String>,
    pub review_round_id: Option<String>,
    pub disposition: String,
    pub failure: pl_protocol::TurnFailure,
    pub resolved_at: Option<i64>,
    pub created_at: i64,
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
    pub execution_status: String,
    pub execution_error: Option<String>,
    pub budget_limit: Option<StudioBudgetLimitRuntime>,
    pub budget_slice_count: u32,
    pub budget_slice_limit: u32,
    pub continuation_state: String,
    pub continuation_source_turn_id: Option<String>,
    pub continuation_revision: u64,
    #[serde(default)]
    pub executor_progress_revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioBudgetLimitRuntime {
    pub kind: String,
    pub usage: StudioBudgetUsageRuntime,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioBudgetUsageRuntime {
    pub model_steps: u32,
    pub tool_calls: u32,
    pub wait_calls: u32,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioAgentProgressRuntime {
    pub stage: String,
    pub summary: String,
    pub next_step: String,
    pub revision: u64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioAgentDirectoryEntry {
    pub id: String,
    pub thread_id: String,
    pub root_thread_id: String,
    pub path: String,
    pub parent_path: Option<String>,
    pub role: String,
    pub task: String,
    pub status: String,
    pub summary: Option<String>,
    pub depth: u32,
    pub error: Option<String>,
    pub reason: Option<String>,
    pub lifecycle: String,
    pub activity: String,
    pub progress: Option<StudioAgentProgressRuntime>,
    pub updated_at: i64,
    pub summary_age_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioTaskCompletionRuntime {
    pub id: String,
    pub work_unit_id: String,
    pub executor_agent_id: String,
    pub revision: u32,
    pub kind: String,
    pub status: String,
    pub base_commit: String,
    pub head_commit: Option<String>,
    pub changed_files: Vec<String>,
    pub verification_summary: String,
    pub worktree_path: String,
    pub branch: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioTaskMergeRuntime {
    pub id: String,
    pub work_unit_id: String,
    pub completion_id: String,
    pub completion_revision: u32,
    pub executor_agent_id: String,
    pub expected_previous_head: String,
    pub resulting_head: String,
    pub delivery_head: String,
    pub method: String,
    pub summary: String,
    pub cleanup_status: String,
    pub cleanup_detail: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioTaskReviewRuntime {
    pub id: String,
    pub round: u32,
    pub scope: String,
    pub work_unit_id: Option<String>,
    pub completion_id: Option<String>,
    pub completion_revision: Option<u32>,
    pub reviewed_head: String,
    pub verdict: String,
    pub requested_by_call_id: String,
    pub reviewer_agent_id: Option<String>,
    pub summary: Option<String>,
    pub design_references: Vec<StudioTaskDesignReferenceRuntime>,
    pub findings: Vec<StudioTaskReviewFindingRuntime>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioTaskDesignReferenceRuntime {
    pub path: String,
    pub section: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioTaskReviewFindingRuntime {
    pub severity: String,
    pub title: String,
    pub body: String,
    pub path: Option<String>,
    pub line: Option<u32>,
    pub design_references: Vec<StudioTaskDesignReferenceRuntime>,
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
