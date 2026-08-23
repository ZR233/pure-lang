use serde::{Deserialize, Serialize};
// ── Observed resource state payloads ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeUninitializedResource {
    pub revision: u64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeLoadingResource {
    pub revision: u64,
    pub operation: BridgeStateOperation,
    pub operation_id: String,
    pub started_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeReadyResource {
    pub revision: u64,
    pub updated_at: i64,
    pub last_checked_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeRefreshingResource {
    pub revision: u64,
    pub operation: BridgeStateOperation,
    pub operation_id: String,
    pub started_at: i64,
    pub last_checked_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeStaleResource {
    pub revision: u64,
    pub stale_at: i64,
    pub last_checked_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeDegradedResource {
    pub revision: u64,
    pub failed_at: i64,
    pub last_checked_at: Option<i64>,
    pub operation: BridgeStateOperation,
    pub error: BridgeStateError,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeFailedResource {
    pub revision: u64,
    pub failed_at: i64,
    pub operation: BridgeStateOperation,
    pub error: BridgeStateError,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeStoppedResource {
    pub revision: u64,
    pub stopped_at: i64,
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
    pub revision: u64,
    pub state: BridgeRuntimeState,
    pub active_turns: Vec<BridgeActiveTurn>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum BridgeRuntimeState {
    Uninitialized(BridgeRuntimeTimestamp),
    Initializing(BridgeRuntimeTimestamp),
    Ready(BridgeRuntimeTimestamp),
    ShuttingDown(BridgeRuntimeTimestamp),
    Stopped(BridgeRuntimeTimestamp),
    Failed(BridgeFailedRuntimeState),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeRuntimeTimestamp {
    pub at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeFailedRuntimeState {
    pub failed_at: i64,
    pub error: BridgeStateError,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeActiveTurn {
    pub thread_id: String,
    pub turn_id: String,
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
pub struct BridgeTaskRecoveryTurnDto {
    pub turn_id: String,
    pub state: BridgeTaskRecoveryTurnState,
    pub updated_at: i64,
    pub item_count: u64,
    pub input_count: u64,
    pub tool_count: u64,
    pub tool_summaries: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeTaskRecoveryTurnState {
    Completed,
    Cancelled,
    Failed,
    BudgetLimited,
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
    pub base_commit: Option<String>,
    pub turns: Vec<BridgeTaskRecoveryTurnDto>,
    pub default_turn_ids: Vec<String>,
    pub available_modes: Vec<BridgeConversationRecoveryMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeTaskRecoveryPreviewDto {
    pub preview_token: String,
    pub root_thread_id: String,
    pub run_id: String,
    pub revision: u64,
    pub task_generation: u64,
    pub state: BridgeTaskRecoveryState,
    pub recommended_thread_id: String,
    pub targets: Vec<BridgeTaskRecoveryTargetDto>,
    pub completion_revision_fingerprint: String,
    pub review_revision_fingerprint: String,
    pub merge_revision_fingerprint: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeTaskRecoveryState {
    Planning,
    PendingConfirmation,
    EditingDocuments,
    Working,
    Reviewing,
    Completed,
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
    pub resume_turn_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeTaskRuntimeDto {
    pub run_id: String,
    pub state: BridgeTaskState,
    pub revision: u64,
    pub generation: u64,
    pub integrated_review_gate: BridgeIntegratedReviewGateDto,
    pub issues: Vec<BridgeTaskIssueDto>,
    pub work_units: Vec<BridgeTaskWorkUnitDto>,
    pub completions: Vec<BridgeTaskCompletionDto>,
    pub merges: Vec<BridgeTaskMergeDto>,
    pub reviews: Vec<BridgeTaskReviewDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum BridgeTaskState {
    Planning(BridgePlanningTaskState),
    PendingConfirmation(BridgePendingConfirmationTaskState),
    EditingDocuments(BridgeEditingDocumentsTaskState),
    Working(BridgeWorkingTaskState),
    Reviewing(BridgeReviewingTaskState),
    Completed(BridgeCompletedTaskState),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgePlanningTaskState {
    pub request: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgePendingConfirmationTaskState {
    pub plan_revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeEditingDocumentsTaskState {
    pub plan_revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeWorkingTaskState {
    pub document_edit_summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeReviewingTaskState {
    pub target: BridgeIntegratedReviewTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeIntegratedReviewTarget {
    pub review_round_id: String,
    pub reviewed_head: String,
    pub changed_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeCompletedTaskState {
    pub outcome: BridgeTaskOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum BridgeTaskOutcome {
    Succeeded {
        summary: String,
        completed_at: i64,
        review_gate: BridgeTaskReviewGate,
    },
    Failed {
        kind: BridgeTaskFailureKind,
        summary: String,
        evidence: String,
        cause: String,
        completed_at: i64,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeTaskFailureKind {
    UnableToProceed,
    Fatal,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum BridgeTaskReviewGate {
    NotRequiredNoDelivery,
    NotRequiredSingleExecutor { work_unit_id: String },
    IntegratedReview { review_round_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "status")]
pub enum BridgeIntegratedReviewGateDto {
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
pub struct BridgeTaskIssueDto {
    pub id: String,
    pub source_thread_id: String,
    pub source_turn_id: String,
    pub source_agent_id: String,
    pub source_role: String,
    pub work_unit_id: Option<String>,
    pub review_round_id: Option<String>,
    pub state: BridgeTaskIssueState,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum BridgeTaskIssueState {
    OpenRecoverable {
        failure: BridgeTaskFailureDetail,
    },
    OpenFatal {
        failure: BridgeTaskFailureDetail,
    },
    Resolved {
        failure: BridgeTaskFailureDetail,
        resolved_at: i64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeTaskFailureDetail {
    pub category: String,
    pub provider_kind: Option<String>,
    pub code: Option<String>,
    pub http_status: Option<u16>,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeTaskWorkUnitDto {
    pub id: String,
    pub title: String,
    pub state: BridgeTaskWorkUnitState,
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
pub enum BridgeTaskWorkUnitState {
    Pending(BridgePendingWorkUnit),
    Running(BridgeRunningWorkUnit),
    WaitingReview(BridgeWaitingReviewWorkUnit),
    ReviewPassed(BridgeReviewPassedWorkUnit),
    ChangesRequired(BridgeChangesRequiredWorkUnit),
    Paused(BridgePausedWorkUnit),
    Completed(BridgeCompletedWorkUnit),
    Failed(BridgeFailedWorkUnit),
    Cancelled(BridgeCancelledWorkUnit),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgePendingWorkUnit {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeRunningWorkUnit {
    pub activity: BridgeRunningWorkUnitActivity,
    pub continuation: BridgeExecutorContinuationState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum BridgeRunningWorkUnitActivity {
    Allocated,
    Active { turn_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeWaitingReviewWorkUnit {
    pub phase: BridgeWaitingReviewPhase,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum BridgeWaitingReviewPhase {
    AwaitingReport {
        outcome: BridgeExecutorTerminalOutcome,
        continuation: BridgeExecutorContinuationState,
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
pub enum BridgeExecutorTerminalOutcome {
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
pub struct BridgeReviewPassedWorkUnit {
    pub completion_id: String,
    pub completion_revision: u32,
    pub review_round_id: String,
    pub outcome: BridgeReviewPassedOutcome,
    pub verification_summary: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeReviewPassedOutcome {
    Delivery,
    NoDelivery,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeChangesRequiredWorkUnit {
    pub completion_id: String,
    pub completion_revision: u32,
    pub review_round_id: String,
    pub continuation_revision: u64,
    pub slice_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgePausedWorkUnit {
    pub reason: BridgeWorkUnitPauseReason,
    pub continuation: BridgeExecutorContinuationState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum BridgeWorkUnitPauseReason {
    Budget {
        limit: BridgeBudgetLimitDto,
    },
    Operational {
        operation_id: String,
        detail: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeCompletedWorkUnit {
    pub outcome: BridgeWorkUnitCompletionOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum BridgeWorkUnitCompletionOutcome {
    Merged { merge_record_id: String },
    NoDelivery { completion_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeFailedWorkUnit {
    pub failure: BridgeWorkUnitFailure,
    pub worktree_disposition: BridgeTaskWorktreeDisposition,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeTaskWorktreeDisposition {
    Protect,
    CleanupRequested,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum BridgeWorkUnitFailure {
    Spawn {
        failure: Box<BridgeTaskSpawnFailure>,
    },
    Execution {
        operation_id: String,
        detail: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeTaskSpawnFailure {
    pub code: BridgeTaskSpawnFailureCode,
    pub phase: BridgeTaskSpawnFailurePhase,
    pub recoverable: bool,
    pub message: String,
    pub task_run_id: Option<String>,
    pub work_unit_id: Option<String>,
    pub agent_id: String,
    pub resource: Option<BridgeTaskSpawnResource>,
    pub cause: BridgeWorktreeFailureCause,
    pub compensation: BridgeTaskSpawnCompensation,
    pub next_action: BridgeTaskSpawnNextAction,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeTaskSpawnFailureCode {
    Allocation,
    WorktreeCreate,
    ChildThreadCreate,
    AgentRegistration,
    Activation,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeTaskSpawnFailurePhase {
    Allocation,
    WorktreeCreate,
    ChildThreadCreate,
    AgentRegistration,
    Activation,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeTaskSpawnNextAction {
    RetryTaskSpawnExecutor,
    RecoverWorktreeResources,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeTaskSpawnResource {
    pub repo_root: String,
    pub path: String,
    pub branch: String,
    pub base_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeWorktreeFailureCause {
    pub kind: BridgeWorktreeFailureCauseKind,
    pub message: String,
    pub args: Option<String>,
    pub exit_code: Option<i32>,
    pub stderr: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeWorktreeFailureCauseKind {
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
pub struct BridgeTaskSpawnCompensation {
    pub allocation: BridgeTaskSpawnCompensationState,
    pub worktree: BridgeTaskSpawnCompensationState,
    pub child_thread: BridgeTaskSpawnCompensationState,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeTaskSpawnCompensationState {
    NotCreated,
    MarkedFailed,
    Removed,
    Faulted,
    CleanupFailed,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeCancelledWorkUnit {
    pub operation_id: String,
    pub reason: String,
    pub worktree_disposition: BridgeTaskWorktreeDisposition,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum BridgeExecutorContinuationState {
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
        limit: BridgeBudgetLimitDto,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeBudgetLimitDto {
    pub kind: BridgeBudgetLimitKind,
    pub usage: BridgeBudgetUsageDto,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeBudgetLimitKind {
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum BridgeAgentState {
    Idle(BridgeIdleAgent),
    Queued(BridgeQueuedAgent),
    Running(BridgeRunningAgent),
    WaitingTool(BridgeWaitingToolAgent),
    WaitingInteraction(BridgeWaitingInteractionAgent),
    Cancelling(BridgeCancellingAgent),
    Closing(BridgeClosingAgent),
    Closed(BridgeClosedAgent),
    Faulted(BridgeFaultedAgent),
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct BridgeIdleAgent {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeQueuedAgent {
    pub turn_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeRunningAgent {
    pub turn_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeWaitingToolAgent {
    pub turn_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeWaitingInteractionAgent {
    pub turn_id: String,
    pub interaction_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeCancellingAgent {
    pub turn_id: String,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct BridgeClosingAgent {}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct BridgeClosedAgent {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeFaultedAgent {
    pub error: BridgeStateError,
    pub diagnostic_turn_id: Option<String>,
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
    pub summary: Option<String>,
    pub depth: u32,
    pub state: BridgeAgentState,
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
    pub content: BridgeTaskCompletionContent,
    pub state: BridgeTaskCompletionState,
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
pub enum BridgeTaskCompletionContent {
    Delivery(BridgeTaskDeliveryCompletion),
    NoDelivery(BridgeTaskNoDeliveryCompletion),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeTaskDeliveryCompletion {
    pub head_commit: String,
    pub changed_files: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeTaskNoDeliveryCompletion {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum BridgeTaskCompletionState {
    ReadyForReview(BridgeReadyForReviewCompletion),
    ChangesRequired(BridgeReviewedCompletion),
    Approved(BridgeReviewedCompletion),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeReadyForReviewCompletion {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeReviewedCompletion {
    pub review_round_id: String,
    pub decided_at: i64,
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
    pub method: BridgeMergeMethod,
    pub summary: String,
    pub cleanup: BridgeMergeCleanupState,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeMergeMethod {
    Merge,
    CherryPick,
    Squash,
    Rebase,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum BridgeMergeCleanupState {
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
pub struct BridgeTaskReviewDto {
    pub id: String,
    pub round: u32,
    pub scope: BridgeReviewScope,
    pub work_unit_id: Option<String>,
    pub completion_id: Option<String>,
    pub completion_revision: Option<u32>,
    pub reviewed_head: String,
    pub state: BridgeTaskReviewState,
    pub requested_by_call_id: String,
    pub design_references: Vec<BridgeTaskDesignReferenceDto>,
    pub findings: Vec<BridgeTaskReviewFindingDto>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeReviewScope {
    Delivery,
    Integrated,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum BridgeTaskReviewState {
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
    pub transport: String,
    pub endpoint: String,
    pub source_kind: String,
    pub mutation_policy: String,
    pub state: BridgeMcpServerState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum BridgeMcpServerState {
    Disabled {
        message: String,
    },
    MissingCredential {
        message: String,
    },
    Checking {
        message: String,
    },
    Available {
        checked_at: i64,
        tool_count: u64,
    },
    Unavailable {
        checked_at: i64,
        error: BridgeStateError,
    },
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
    pub state: BridgeLspServerState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum BridgeLspServerState {
    Checking {
        message: String,
    },
    Available {
        checked_at: i64,
        diagnostic_count: u64,
        activity: BridgeLspActivity,
    },
    Unavailable {
        checked_at: i64,
        error: BridgeStateError,
    },
    Disabled {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum BridgeLspActivity {
    Idle,
    Busy {
        title: Option<String>,
        message: Option<String>,
        percentage: Option<u32>,
    },
    Indexing {
        title: Option<String>,
        message: Option<String>,
        percentage: Option<u32>,
    },
}
