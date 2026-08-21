use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioTaskRuntime {
    pub run_id: String,
    pub state: StudioTaskState,
    pub branch: String,
    pub expected_head: String,
    pub revision: u64,
    pub integrated_review_gate: StudioIntegratedReviewGate,
    pub failures: Vec<StudioTaskFailureRuntime>,
    pub terminal_failure: Option<StudioTaskFailureRuntime>,
    pub work_units: Vec<StudioTaskWorkUnitRuntime>,
    pub completions: Vec<StudioTaskCompletionRuntime>,
    pub merges: Vec<StudioTaskMergeRuntime>,
    pub reviews: Vec<StudioTaskReviewRuntime>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum StudioTaskState {
    DesignUpdating(StudioTaskStateData),
    Implementing(StudioTaskStateData),
    Merging(StudioTaskStateData),
    Reviewing(StudioTaskStateData),
    Reworking(StudioTaskStateData),
    Stopping(StudioTaskStateData),
    Blocked(StudioTaskStateData),
    Completed(StudioTaskStateData),
    Failed(StudioTaskStateData),
    Cancelled(StudioTaskStateData),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioTaskStateData {
    pub generation: u64,
    pub status_message: Option<String>,
    pub finalized_design: Option<StudioFinalizedDesign>,
    pub stop_request: Option<StudioTaskStopRequest>,
    pub review_target: Option<StudioTaskReviewTarget>,
    pub blocked_recovery: Option<StudioBlockedRecovery>,
    pub failure_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioFinalizedDesign {
    pub head: String,
    pub commit: Option<String>,
    pub summary: String,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioTaskStopRequest {
    pub origin: String,
    pub reason: String,
    pub requested_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum StudioTaskReviewTarget {
    Delivery {
        work_unit_id: String,
        completion_id: String,
        completion_revision: u32,
        reviewed_head: String,
    },
    Integration {
        reviewed_head: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioBlockedRecovery {
    RetryMerge,
    ResumeRework,
    ManualOnly,
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
    pub state: StudioTaskWorkUnitState,
    pub worktree_path: String,
    pub branch: String,
    pub agent_id: Option<String>,
    pub budget_slice_limit: u32,
    #[serde(default)]
    pub executor_progress_revision: u64,
    pub blueprint_fingerprint: Option<String>,
    pub objective: Option<String>,
    pub implementation_step_count: usize,
    pub acceptance_criterion_count: usize,
    pub verification_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum StudioTaskWorkUnitState {
    Pending(StudioTaskWorkUnitProgress),
    Running(StudioRunningWorkUnit),
    AwaitingCompletion(StudioAwaitingWorkUnit),
    ReadyForReview(StudioTaskWorkUnitProgress),
    Reviewing(StudioTaskWorkUnitProgress),
    ChangesRequested(StudioTaskWorkUnitProgress),
    Approved(StudioTaskWorkUnitProgress),
    Merged(StudioTaskWorkUnitProgress),
    NoDelivery(StudioTaskWorkUnitProgress),
    NeedsAttention(StudioTaskWorkUnitProgress),
    Failed(StudioTaskWorkUnitProgress),
    Cancelled(StudioTaskWorkUnitProgress),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioRunningWorkUnit {
    pub execution: StudioRunningExecution,
    pub progress: StudioTaskWorkUnitProgress,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioAwaitingWorkUnit {
    pub execution: StudioAwaitingExecution,
    pub progress: StudioTaskWorkUnitProgress,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioRunningExecution {
    Running,
    BudgetLimited,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioAwaitingExecution {
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioTaskWorkUnitProgress {
    pub worktree_disposition: StudioTaskWorktreeDisposition,
    pub execution_summary: Option<String>,
    pub execution_error: Option<String>,
    pub budget_limit: Option<StudioBudgetLimitRuntime>,
    pub budget_slice_count: u32,
    pub continuation_state: StudioExecutorContinuationState,
    pub continuation_source_turn_id: Option<String>,
    pub continuation_revision: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioTaskWorktreeDisposition {
    Protect,
    CleanupRequested,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioExecutorContinuationState {
    None,
    PendingStart,
    Compacting,
    PlannerWakePending,
    NeedsAttention,
}

/// 当前任务是否还需要综合审查，以及已满足门禁时的可审计依据。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "status")]
pub enum StudioIntegratedReviewGate {
    Required {
        reason: String,
    },
    SatisfiedByReview {
        review_round_id: String,
        reviewed_head: String,
    },
    NotRequiredNoDelivery,
    NotRequiredSingleExecutorEquivalent {
        work_unit_id: String,
        completion_revision: u32,
        merge_record_id: String,
    },
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioAgentActivity {
    Idle,
    Queued,
    ActiveRunning,
    ActiveWaitingTool,
    ActiveWaitingInteraction,
    Cancelling,
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
    pub activity: StudioAgentActivity,
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
    pub state: StudioTaskReviewState,
    pub requested_by_call_id: String,
    pub reviewer_agent_id: Option<String>,
    pub design_references: Vec<StudioTaskDesignReferenceRuntime>,
    pub findings: Vec<StudioTaskReviewFindingRuntime>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum StudioTaskReviewState {
    Pending {
        reviewer: StudioPendingReviewerState,
    },
    Pass {
        summary: String,
    },
    ChangesRequired {
        summary: String,
    },
    Blocked {
        summary: String,
    },
    Failed {
        reviewer: StudioFailedReviewerState,
        error: String,
        summary: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioPendingReviewerState {
    Queued,
    Running,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioFailedReviewerState {
    Failed,
    Cancelled,
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
    #[serde(default)]
    pub recommendation: String,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioMcpServer {
    pub id: String,
    pub enabled: bool,
    pub transport: String,
    pub endpoint: String,
    pub source_kind: String,
    pub status_kind: String,
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
