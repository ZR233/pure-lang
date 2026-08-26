use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioTaskRuntime {
    pub run_id: String,
    pub state: StudioTaskState,
    pub revision: u64,
    pub generation: u64,
    pub integrated_review_gate: StudioIntegratedReviewGate,
    pub issues: Vec<StudioTaskIssueRuntime>,
    pub work_units: Vec<StudioTaskWorkUnitRuntime>,
    pub completions: Vec<StudioTaskCompletionRuntime>,
    pub merges: Vec<StudioTaskMergeRuntime>,
    pub reviews: Vec<StudioTaskReviewRuntime>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum StudioTaskState {
    Planning(StudioPlanningTaskState),
    PendingConfirmation(StudioPendingConfirmationTaskState),
    EditingDocuments(StudioEditingDocumentsTaskState),
    Working(StudioWorkingTaskState),
    Reviewing(StudioReviewingTaskState),
    Completed(StudioCompletedTaskState),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioPlanningTaskState {
    pub request: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioPendingConfirmationTaskState {
    pub plan_revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioEditingDocumentsTaskState {
    pub plan_revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioWorkingTaskState {
    pub document_edit_summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioReviewingTaskState {
    pub target: StudioIntegratedReviewTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioIntegratedReviewTarget {
    pub review_round_id: String,
    pub reviewed_head: String,
    pub changed_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioCompletedTaskState {
    pub outcome: StudioTaskOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum StudioTaskOutcome {
    Succeeded {
        summary: String,
        completed_at: i64,
        review_gate: StudioTaskReviewGate,
    },
    Failed {
        kind: StudioTaskFailureKind,
        summary: String,
        evidence: String,
        cause: String,
        completed_at: i64,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioTaskFailureKind {
    UnableToProceed,
    Fatal,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum StudioTaskReviewGate {
    NotRequiredNoDelivery,
    NotRequiredSingleExecutor { work_unit_id: String },
    IntegratedReview { review_round_id: String },
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioTaskIssueRuntime {
    pub id: String,
    pub source_thread_id: String,
    pub source_turn_id: String,
    pub source_agent_id: String,
    pub source_role: String,
    pub work_unit_id: Option<String>,
    pub review_round_id: Option<String>,
    pub state: StudioTaskIssueState,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum StudioTaskIssueState {
    OpenRecoverable {
        failure: pl_protocol::TurnFailure,
    },
    OpenFatal {
        failure: pl_protocol::TurnFailure,
    },
    Resolved {
        failure: pl_protocol::TurnFailure,
        resolved_at: i64,
    },
}

impl StudioTaskIssueState {
    pub const fn resolved_at(&self) -> Option<i64> {
        match self {
            Self::OpenRecoverable { .. } | Self::OpenFatal { .. } => None,
            Self::Resolved { resolved_at, .. } => Some(*resolved_at),
        }
    }
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
    pub attempt: u32,
    pub supersedes_work_unit_id: Option<String>,
    pub budget_slice_limit: u32,
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
    Pending(StudioPendingWorkUnit),
    Running(StudioRunningWorkUnit),
    WaitingReview(StudioWaitingReviewWorkUnit),
    ReviewPassed(StudioReviewPassedWorkUnit),
    ChangesRequired(StudioChangesRequiredWorkUnit),
    Paused(StudioPausedWorkUnit),
    Completed(StudioCompletedWorkUnit),
    Failed(StudioFailedWorkUnit),
    Cancelled(StudioCancelledWorkUnit),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioPendingWorkUnit {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioRunningWorkUnit {
    pub activity: StudioRunningWorkUnitActivity,
    pub continuation: StudioExecutorContinuationState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum StudioRunningWorkUnitActivity {
    Allocated,
    Active { turn_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioWaitingReviewWorkUnit {
    pub phase: StudioWaitingReviewPhase,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum StudioWaitingReviewPhase {
    AwaitingReport {
        outcome: StudioExecutorTerminalOutcome,
        continuation: StudioExecutorContinuationState,
    },
    Ready {
        completion_id: String,
        completion_revision: u32,
        verification_summary: String,
    },
    Reviewing {
        completion_id: String,
        completion_revision: u32,
        review_round_id: String,
        verification_summary: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum StudioExecutorTerminalOutcome {
    Completed {
        source_turn_id: String,
        detail: String,
    },
    Failed {
        source_turn_id: String,
        detail: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioReviewPassedWorkUnit {
    pub completion_id: String,
    pub completion_revision: u32,
    pub review_round_id: String,
    pub outcome: StudioReviewPassedOutcome,
    pub verification_summary: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioReviewPassedOutcome {
    Delivery,
    NoDelivery,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioChangesRequiredWorkUnit {
    pub completion_id: String,
    pub completion_revision: u32,
    pub review_round_id: String,
    pub continuation_revision: u64,
    pub slice_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioPausedWorkUnit {
    pub reason: StudioWorkUnitPauseReason,
    pub continuation: StudioExecutorContinuationState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum StudioWorkUnitPauseReason {
    Budget {
        limit: StudioBudgetLimitRuntime,
    },
    Operational {
        operation_id: String,
        detail: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioCompletedWorkUnit {
    pub outcome: StudioWorkUnitCompletionOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum StudioWorkUnitCompletionOutcome {
    Merged { merge_record_id: String },
    NoDelivery { completion_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioFailedWorkUnit {
    pub failure: StudioWorkUnitFailure,
    pub worktree_disposition: StudioTaskWorktreeDisposition,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioTaskWorktreeDisposition {
    Protect,
    CleanupRequested,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum StudioWorkUnitFailure {
    Spawn {
        failure: Box<StudioTaskSpawnFailure>,
    },
    Execution {
        operation_id: String,
        detail: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioTaskSpawnFailure {
    pub code: StudioTaskSpawnFailureCode,
    pub phase: StudioTaskSpawnFailurePhase,
    pub recoverable: bool,
    pub message: String,
    pub task_run_id: Option<String>,
    pub work_unit_id: Option<String>,
    pub agent_id: String,
    pub resource: Option<StudioTaskSpawnResource>,
    pub cause: StudioWorktreeFailureCause,
    pub compensation: StudioTaskSpawnCompensation,
    pub next_action: StudioTaskSpawnNextAction,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioTaskSpawnFailureCode {
    Allocation,
    WorktreeCreate,
    ChildThreadCreate,
    AgentRegistration,
    Activation,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioTaskSpawnFailurePhase {
    Allocation,
    WorktreeCreate,
    ChildThreadCreate,
    AgentRegistration,
    Activation,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioTaskSpawnNextAction {
    RetryTaskSpawnExecutor,
    RecoverWorktreeResources,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioTaskSpawnResource {
    pub repo_root: String,
    pub path: String,
    pub branch: String,
    pub base_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioWorktreeFailureCause {
    pub kind: StudioWorktreeFailureCauseKind,
    pub message: String,
    pub args: Option<String>,
    pub exit_code: Option<i32>,
    pub stderr: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioWorktreeFailureCauseKind {
    InvalidRepoRoot,
    UnsafeBranch,
    GitLaunchFailed,
    GitTimedOut,
    GitExited,
    GitStatusUnknown,
    Io,
    Disabled,
    OperationAndCleanupFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioTaskSpawnCompensation {
    pub allocation: StudioTaskSpawnCompensationState,
    pub worktree: StudioTaskSpawnCompensationState,
    pub child_thread: StudioTaskSpawnCompensationState,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioTaskSpawnCompensationState {
    NotCreated,
    MarkedFailed,
    Removed,
    Faulted,
    CleanupFailed,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioCancelledWorkUnit {
    pub operation_id: String,
    pub reason: String,
    pub worktree_disposition: StudioTaskWorktreeDisposition,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum StudioExecutorContinuationState {
    Idle {
        revision: u64,
        slice_count: u32,
    },
    Compacting {
        revision: u64,
        source_turn_id: String,
        slice_count: u32,
    },
    PendingStart {
        revision: u64,
        source_turn_id: String,
        slice_count: u32,
        limit: StudioBudgetLimitRuntime,
    },
    PlannerWakePending {
        revision: u64,
        source_turn_id: String,
        slice_count: u32,
    },
    NeedsAttention {
        revision: u64,
        source_turn_id: String,
        slice_count: u32,
        detail: String,
    },
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
    pub kind: StudioBudgetLimitKind,
    pub usage: StudioBudgetUsageRuntime,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioBudgetLimitKind {
    ModelStep,
    ToolCall,
    Wait,
    WallClock,
    AgentCount,
    AgentDepth,
    Finalization,
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
pub struct StudioAgentDirectoryEntry {
    pub id: String,
    pub thread_id: String,
    pub root_thread_id: String,
    pub path: String,
    pub parent_path: Option<String>,
    pub role: String,
    pub task: String,
    pub summary: Option<String>,
    pub depth: u32,
    pub state: pl_protocol::AgentState,
    pub progress: Option<pl_protocol::AgentProgressCheckpoint>,
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
    pub content: StudioTaskCompletionContent,
    pub state: StudioTaskCompletionState,
    pub state_revision: u64,
    pub base_commit: String,
    pub verification_summary: String,
    pub worktree_path: String,
    pub branch: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum StudioTaskCompletionContent {
    Delivery(StudioTaskDeliveryCompletion),
    NoDelivery(StudioTaskNoDeliveryCompletion),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioTaskDeliveryCompletion {
    pub head_commit: String,
    pub changed_files: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioTaskNoDeliveryCompletion {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum StudioTaskCompletionState {
    ReadyForReview(StudioReadyForReviewCompletion),
    ChangesRequired(StudioReviewedCompletion),
    Approved(StudioReviewedCompletion),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioReadyForReviewCompletion {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioReviewedCompletion {
    pub review_round_id: String,
    pub decided_at: i64,
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
    pub method: StudioMergeMethod,
    pub summary: String,
    pub cleanup: StudioMergeCleanupState,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioMergeMethod {
    Merge,
    CherryPick,
    Squash,
    Rebase,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum StudioMergeCleanupState {
    Pending,
    Deferred,
    Attempting {
        operation_id: String,
        started_at: i64,
    },
    Discarded {
        operation_id: String,
        completed_at: i64,
    },
    AlreadyAbsent {
        operation_id: String,
        completed_at: i64,
    },
    Failed {
        operation_id: String,
        failed_at: i64,
        detail: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioTaskReviewRuntime {
    pub id: String,
    pub round: u32,
    pub scope: StudioReviewScope,
    pub work_unit_id: Option<String>,
    pub completion_id: Option<String>,
    pub completion_revision: Option<u32>,
    pub reviewed_head: String,
    pub state: StudioTaskReviewState,
    pub requested_by_call_id: String,
    pub design_references: Vec<StudioTaskDesignReferenceRuntime>,
    pub findings: Vec<StudioTaskReviewFindingRuntime>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioReviewScope {
    Delivery,
    Integrated,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum StudioTaskReviewState {
    PendingDispatch,
    Dispatched {
        reviewer_agent_id: String,
    },
    Running {
        reviewer_agent_id: String,
    },
    Passed {
        reviewer_agent_id: String,
        summary: String,
    },
    ChangesRequired {
        reviewer_agent_id: String,
        summary: String,
    },
    Blocked {
        reviewer_agent_id: String,
        summary: String,
    },
    Failed {
        reviewer_agent_id: Option<String>,
        error: String,
        summary: String,
    },
    Cancelled {
        reviewer_agent_id: Option<String>,
        reason: String,
        summary: String,
    },
}

impl StudioTaskReviewState {
    pub fn reviewer_agent_id(&self) -> Option<&str> {
        match self {
            Self::PendingDispatch => None,
            Self::Dispatched { reviewer_agent_id }
            | Self::Running { reviewer_agent_id }
            | Self::Passed {
                reviewer_agent_id, ..
            }
            | Self::ChangesRequired {
                reviewer_agent_id, ..
            }
            | Self::Blocked {
                reviewer_agent_id, ..
            } => Some(reviewer_agent_id),
            Self::Failed {
                reviewer_agent_id, ..
            }
            | Self::Cancelled {
                reviewer_agent_id, ..
            } => reviewer_agent_id.as_deref(),
        }
    }
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
    pub transport: String,
    pub endpoint: String,
    pub source_kind: String,
    pub mutation_policy: String,
    pub state: crate::StudioMcpServerState,
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
    pub state: crate::StudioLspServerState,
}
