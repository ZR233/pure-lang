use serde::{Deserialize, Serialize};
// ── Observed state types ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeObservedStateMeta {
    pub revision: u64,
    pub phase: BridgeObservedStatePhase,
    pub updated_at: i64,
    pub last_checked_at: Option<i64>,
    pub stale: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeObservedStatePhase {
    Uninitialized,
    Ready,
    Running {
        operation: BridgeStateOperation,
        operation_id: String,
    },
    Failed {
        operation: BridgeStateOperation,
        error: BridgeStateError,
    },
    Stopped,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeStateOperation {
    Initialize,
    Activate,
    Reload,
    Reconcile,
    Discover,
    Check,
    Probe,
    Repair,
    Reset,
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStateError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

// ── Runtime types ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSnapshot {
    pub status: BridgeRuntimeStatus,
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
pub enum BridgeRecoveryIssueScope {
    Application,
    Project,
    Thread,
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
    CleanupThread,
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
    pub thread_id: Option<String>,
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
    pub thread_id: Option<String>,
    pub detail: String,
    pub resources: Vec<BridgeRecoveryCleanupResourceDto>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeConversationRecoveryMode {
    RewindTail,
    RebuildThread,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeTaskRecoveryTargetKind {
    Planner,
    Executor,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeTaskGitFingerprintDto {
    pub workspace_root: String,
    pub git_common_dir: String,
    pub branch: String,
    pub head: String,
    pub base_commit: String,
    pub expected_head: String,
    pub operation: String,
    pub index_diff_hash: String,
    pub working_tree_diff_hash: String,
    pub untracked_content_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeTaskRecoveryTurnDto {
    pub turn_id: String,
    pub status: String,
    pub updated_at: i64,
    pub item_count: u64,
    pub input_count: u64,
    pub tool_count: u64,
    pub tool_summaries: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeTaskRecoveryTargetDto {
    pub thread_id: String,
    pub kind: BridgeTaskRecoveryTargetKind,
    pub work_unit_id: Option<String>,
    pub attempt: Option<u32>,
    pub continuation_revision: Option<u64>,
    pub expected_runtime_revision: u64,
    pub expected_thread_revision: u64,
    pub branch: String,
    pub worktree_path: String,
    pub turns: Vec<BridgeTaskRecoveryTurnDto>,
    pub default_turn_ids: Vec<String>,
    pub available_modes: Vec<BridgeConversationRecoveryMode>,
    pub git_fingerprint: BridgeTaskGitFingerprintDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeTaskRecoveryPreviewDto {
    pub preview_token: String,
    pub root_thread_id: String,
    pub run_id: String,
    pub task_generation: u64,
    pub phase: String,
    pub expected_head: String,
    pub stop_requested: bool,
    pub branch_lease_id: String,
    pub branch_lease_branch: String,
    pub branch_lease_git_common_dir: String,
    pub branch_lease_expected_head: String,
    pub recommended_thread_id: String,
    pub targets: Vec<BridgeTaskRecoveryTargetDto>,
    pub main_git_fingerprint: BridgeTaskGitFingerprintDto,
    pub completion_revision_fingerprint: String,
    pub review_revision_fingerprint: String,
    pub merge_revision_fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeTaskRecoveryRequestDto {
    pub recovery_id: String,
    pub root_thread_id: String,
    pub target_thread_id: String,
    pub mode: BridgeConversationRecoveryMode,
    pub turn_ids: Vec<String>,
    pub preview: BridgeTaskRecoveryPreviewDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeTaskRecoveryResultDto {
    pub recovery_id: String,
    pub run_id: String,
    pub work_unit_id: Option<String>,
    pub root_thread_id: String,
    pub target_thread_id: String,
    pub mode: BridgeConversationRecoveryMode,
    pub recovery_revision: u64,
    pub runtime_revision: u64,
    pub thread_revision: u64,
    pub before_transcript_hash: String,
    pub after_transcript_hash: String,
    pub removed_item_count: u64,
    pub removed_input_count: u64,
    pub stop_cleared: bool,
    pub resume_turn_id: String,
    pub git_fingerprint: BridgeTaskGitFingerprintDto,
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
    pub failures: Vec<BridgeTaskFailureDto>,
    pub terminal_failure: Option<BridgeTaskFailureDto>,
    pub work_units: Vec<BridgeTaskWorkUnitDto>,
    pub completions: Vec<BridgeTaskCompletionDto>,
    pub merges: Vec<BridgeTaskMergeDto>,
    pub reviews: Vec<BridgeTaskReviewDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeTaskFailureDto {
    pub id: String,
    pub source_thread_id: String,
    pub source_turn_id: String,
    pub source_agent_id: String,
    pub source_role: String,
    pub work_unit_id: Option<String>,
    pub review_round_id: Option<String>,
    pub disposition: String,
    pub category: String,
    pub provider_kind: Option<String>,
    pub code: Option<String>,
    pub http_status: Option<u16>,
    pub message: String,
    pub retryable: bool,
    pub resolved_at: Option<i64>,
    pub created_at: i64,
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
    pub execution_status: String,
    pub execution_error: Option<String>,
    pub budget_limit: Option<BridgeBudgetLimitDto>,
    pub budget_slice_count: u32,
    pub budget_slice_limit: u32,
    pub continuation_state: String,
    pub continuation_source_turn_id: Option<String>,
    pub continuation_revision: u64,
    pub executor_progress_revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeBudgetLimitDto {
    pub kind: String,
    pub usage: BridgeBudgetUsageDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeBudgetUsageDto {
    pub model_steps: u32,
    pub tool_calls: u32,
    pub wait_calls: u32,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeAgentProgressDto {
    pub stage: String,
    pub summary: String,
    pub next_step: String,
    pub revision: u64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeAgentActivity {
    Idle,
    Queued,
    ActiveRunning,
    ActiveWaitingTool,
    ActiveWaitingInteraction,
    Cancelling,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeAgentDirectoryEntryDto {
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
    pub activity: BridgeAgentActivity,
    pub progress: Option<BridgeAgentProgressDto>,
    pub updated_at: i64,
    pub summary_age_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeTaskCompletionDto {
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
pub struct BridgeTaskMergeDto {
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
pub struct BridgeTaskReviewDto {
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
    pub design_references: Vec<BridgeTaskDesignReferenceDto>,
    pub findings: Vec<BridgeTaskReviewFindingDto>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeTaskDesignReferenceDto {
    pub path: String,
    pub section: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeTaskReviewFindingDto {
    pub severity: String,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub recommendation: String,
    pub path: Option<String>,
    pub line: Option<u32>,
    pub design_references: Vec<BridgeTaskDesignReferenceDto>,
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
    pub endpoint: String,
    pub source_kind: String,
    pub status_kind: String,
    pub mutation_policy: String,
    pub availability_kind: String,
    pub availability_message: Option<String>,
    pub last_checked_at: Option<i64>,
    pub tool_count: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeLspHealthDto {
    pub active_lsp_servers: Vec<String>,
    pub lsp_servers: Vec<BridgeLspServerDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeLspServerDto {
    pub id: String,
    pub display_name: String,
    pub extensions: Vec<String>,
    pub language_ids: Vec<String>,
    pub availability_kind: String,
    pub availability_message: Option<String>,
    pub last_checked_at: Option<i64>,
    pub diagnostic_count: u64,
    pub activity_kind: String,
    pub activity_title: Option<String>,
    pub activity_message: Option<String>,
    pub activity_percentage: Option<u32>,
    pub last_error: Option<String>,
    pub last_error_at: Option<i64>,
}
