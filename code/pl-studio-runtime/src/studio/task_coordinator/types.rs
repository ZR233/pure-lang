use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{TaskGitFingerprint, TaskRun, TaskRunStateKind, WorkUnit};

mod merge;
pub(crate) use merge::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum TaskFailureDisposition {
    Recoverable,
    Fatal,
}

impl TaskFailureDisposition {
    pub(crate) fn for_turn_failure(failure: &pl_protocol::TurnFailure) -> Self {
        use pl_protocol::{ProviderFailureKind, TurnFailureCategory};

        match failure.category {
            TurnFailureCategory::ProviderCapacity | TurnFailureCategory::Validation => {
                Self::Recoverable
            }
            TurnFailureCategory::Provider
                if failure.retry.is_retryable()
                    || matches!(
                        failure.provider_kind,
                        Some(ProviderFailureKind::Capacity | ProviderFailureKind::Transport)
                    ) =>
            {
                Self::Recoverable
            }
            TurnFailureCategory::Provider
            | TurnFailureCategory::Tool
            | TurnFailureCategory::Internal => Self::Fatal,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Recoverable => "recoverable",
            Self::Fatal => "fatal",
        }
    }

    pub(crate) fn from_str(value: &str) -> Option<Self> {
        match value {
            "recoverable" => Some(Self::Recoverable),
            "fatal" => Some(Self::Fatal),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskFailureRecord {
    pub(crate) id: String,
    pub(crate) task_run_id: String,
    pub(crate) source_thread_id: String,
    pub(crate) source_turn_id: String,
    pub(crate) source_agent_id: String,
    pub(crate) source_role: String,
    pub(crate) work_unit_id: Option<String>,
    pub(crate) review_round_id: Option<String>,
    pub(crate) disposition: TaskFailureDisposition,
    pub(crate) failure: pl_protocol::TurnFailure,
    pub(crate) resolved_at: Option<i64>,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
}

pub(crate) struct RecordTaskAgentFailure {
    pub(crate) root_thread_id: String,
    pub(crate) source_thread_id: String,
    pub(crate) source_turn_id: String,
    pub(crate) source_agent_id: String,
    pub(crate) source_role: String,
    pub(crate) failure: pl_protocol::TurnFailure,
}

pub(crate) struct TaskFailureSettlement {
    pub(crate) run: TaskRun,
    pub(crate) terminalized: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum TaskStopOrigin {
    UserRequest,
    PlannerDecision,
    RuntimeFailure,
    ApplicationShutdown,
}

impl TaskStopOrigin {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::UserRequest => "userRequest",
            Self::PlannerDecision => "plannerDecision",
            Self::RuntimeFailure => "runtimeFailure",
            Self::ApplicationShutdown => "applicationShutdown",
        }
    }

    pub(crate) fn stops_root_turn(self) -> bool {
        !matches!(self, Self::PlannerDecision)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct TaskStopReason(String);

impl TaskStopReason {
    pub(crate) fn new(value: impl Into<String>) -> Option<Self> {
        let value = value.into();
        let value = value.trim();
        (!value.is_empty()).then(|| Self(value.to_string()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::ops::Deref for TaskStopReason {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BranchLeaseRecord {
    pub(crate) id: String,
    pub(crate) task_run_id: String,
    pub(crate) git_common_dir: String,
    pub(crate) branch: String,
    pub(crate) expected_head: String,
    pub(crate) acquired_at: i64,
    pub(crate) updated_at: i64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct RestartAgentReconciliation {
    pub(crate) cancelled_work_units: usize,
    pub(crate) cancelled_thread_executions: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TaskWorktreeOwnerSnapshot {
    pub(crate) run: TaskRun,
    pub(crate) resources: Vec<TaskWorktreeOwnerResource>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskWorktreeCreationState {
    MustExist,
    UncreatedBeforeRestart,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TaskWorktreeCleanupState {
    NotMerged,
    Protect,
    Cleanup,
    Replay { merge_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TaskWorktreeOwnerResource {
    pub(crate) work_unit: WorkUnit,
    pub(crate) completion: Option<WorkCompletionRecord>,
    pub(crate) creation_state: TaskWorktreeCreationState,
    pub(crate) cleanup_state: TaskWorktreeCleanupState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DesignFinalizeOutput {
    pub(crate) task_run_id: String,
    pub(crate) previous_head: String,
    pub(crate) finalized_head: String,
    pub(crate) phase_commit: Option<String>,
    pub(crate) changed_files: Vec<String>,
    pub(crate) state: TaskRunStateKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DesignCancellationRevert {
    pub(crate) task_run_id: String,
    pub(crate) previous_head: String,
    pub(crate) revert_commit: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CreateTaskRun {
    pub(crate) root_thread_id: String,
    pub(crate) plan: String,
    pub(crate) workspace_root: String,
    pub(crate) git_common_dir: String,
    pub(crate) branch: String,
    pub(crate) head_commit: String,
    pub(crate) design_baseline: TaskGitFingerprint,
}

#[cfg(test)]
pub(crate) fn test_task_git_fingerprint(
    workspace_root: impl Into<String>,
    git_common_dir: impl Into<String>,
    branch: impl Into<String>,
    head: impl Into<String>,
) -> TaskGitFingerprint {
    let head = head.into();
    TaskGitFingerprint {
        workspace_root: workspace_root.into(),
        git_common_dir: git_common_dir.into(),
        branch: branch.into(),
        head: head.clone(),
        base_commit: head.clone(),
        expected_head: head,
        operation: "none".to_string(),
        index_diff_hash: "empty-index".to_string(),
        working_tree_diff_hash: "empty-worktree".to_string(),
        untracked_content_hash: "empty-untracked".to_string(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum WorkUnitStatus {
    Pending,
    Running,
    AwaitingCompletion,
    ReadyForReview,
    Reviewing,
    ChangesRequested,
    Approved,
    Merged,
    NoDelivery,
    NeedsAttention,
    Failed,
    Cancelled,
}

impl WorkUnitStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::AwaitingCompletion => "awaitingCompletion",
            Self::ReadyForReview => "readyForReview",
            Self::Reviewing => "reviewing",
            Self::ChangesRequested => "changesRequested",
            Self::Approved => "approved",
            Self::Merged => "merged",
            Self::NoDelivery => "noDelivery",
            Self::NeedsAttention => "needsAttention",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub(crate) fn from_str(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "running" => Some(Self::Running),
            "awaitingCompletion" => Some(Self::AwaitingCompletion),
            "readyForReview" => Some(Self::ReadyForReview),
            "reviewing" => Some(Self::Reviewing),
            "changesRequested" => Some(Self::ChangesRequested),
            "approved" => Some(Self::Approved),
            "merged" => Some(Self::Merged),
            "noDelivery" => Some(Self::NoDelivery),
            "needsAttention" => Some(Self::NeedsAttention),
            "failed" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ThreadExecutionStatus {
    Queued,
    Running,
    Completed,
    BudgetLimited,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExecutorCloseDisposition {
    PreserveForMerge,
    Discard,
}

impl ThreadExecutionStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::BudgetLimited => "budgetLimited",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ExecutorContinuationState {
    #[default]
    None,
    Compacting,
    PendingStart,
    PlannerWakePending,
    NeedsAttention,
}

pub(crate) const MAX_EXECUTOR_BUDGET_SLICES: u32 = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExecutorContinuationRequest {
    pub(crate) agent_id: String,
    pub(crate) work_unit_id: String,
    pub(crate) source_turn_id: String,
    pub(crate) slice_count: u32,
}

impl ExecutorContinuationRequest {
    pub(crate) fn mail_id(&self) -> String {
        format!(
            "task-executor-continuation:{}:{}",
            self.work_unit_id, self.source_turn_id
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TaskPlannerWakeSource {
    Review {
        review_round_id: String,
        scope: ReviewScope,
    },
    ExecutorTerminal {
        work_unit_id: String,
        executor_thread_id: String,
        source_turn_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TaskPlannerWakeRequest {
    pub(crate) task_run_id: String,
    pub(crate) root_thread_id: String,
    pub(crate) source: TaskPlannerWakeSource,
}

impl TaskPlannerWakeRequest {
    pub(crate) fn mail_id(&self) -> String {
        match &self.source {
            TaskPlannerWakeSource::Review {
                review_round_id, ..
            } => format!("task-review-continuation:{review_round_id}"),
            TaskPlannerWakeSource::ExecutorTerminal {
                work_unit_id,
                source_turn_id,
                ..
            } => format!("task-executor-terminal:{work_unit_id}:{source_turn_id}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ReviewVerdict {
    Pending,
    Pass,
    ChangesRequired,
    Blocked,
    Failed,
}

impl ReviewVerdict {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Pass => "pass",
            Self::ChangesRequired => "changesRequired",
            Self::Blocked => "blocked",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentWorktreeDelivery {
    pub(crate) path: String,
    pub(crate) branch: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentDelivery {
    pub(crate) worktree: AgentWorktreeDelivery,
    pub(crate) base_commit: String,
    pub(crate) head_commit: String,
    pub(crate) changed_files: Vec<String>,
    pub(crate) verification_summary: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum TaskWorktreeDisposition {
    Protect,
    CleanupRequested,
}

impl TaskWorktreeDisposition {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Protect => "protect",
            Self::CleanupRequested => "cleanupRequested",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum WorkCompletionKind {
    Delivery,
    NoDelivery,
}

impl WorkCompletionKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Delivery => "delivery",
            Self::NoDelivery => "noDelivery",
        }
    }

    pub(crate) fn from_str(value: &str) -> Option<Self> {
        match value {
            "delivery" => Some(Self::Delivery),
            "noDelivery" => Some(Self::NoDelivery),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum WorkCompletionStatus {
    ReadyForReview,
    ChangesRequired,
    Approved,
}

impl WorkCompletionStatus {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::ReadyForReview => "readyForReview",
            Self::ChangesRequired => "changesRequired",
            Self::Approved => "approved",
        }
    }

    pub(crate) fn from_str(value: &str) -> Option<Self> {
        match value {
            "readyForReview" => Some(Self::ReadyForReview),
            "changesRequired" => Some(Self::ChangesRequired),
            "approved" => Some(Self::Approved),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkCompletionRecord {
    pub(crate) id: String,
    pub(crate) task_run_id: String,
    pub(crate) work_unit_id: String,
    pub(crate) executor_agent_id: String,
    pub(crate) revision: u32,
    pub(crate) kind: WorkCompletionKind,
    pub(crate) status: WorkCompletionStatus,
    pub(crate) base_commit: String,
    pub(crate) head_commit: Option<String>,
    pub(crate) changed_files: Vec<String>,
    pub(crate) verification_summary: String,
    pub(crate) worktree_path: String,
    pub(crate) branch: String,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReviewDesignReference {
    /// Normalized design/** path that was actually read.
    pub(crate) path: String,
    /// Section text present in the referenced design file.
    pub(crate) section: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReviewFinding {
    pub(crate) severity: String,
    pub(crate) title: String,
    pub(crate) body: String,
    /// 可执行的修复建议：写清改成什么、为什么，必要时内联代码片段。
    /// 审查者只读，不直接打补丁；executor 据此 rework。
    #[serde(default)]
    pub(crate) recommendation: String,
    #[schemars(required)]
    pub(crate) path: Option<String>,
    #[schemars(required)]
    pub(crate) line: Option<u32>,
    #[serde(default)]
    pub(crate) design_references: Vec<ReviewDesignReference>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentReview {
    pub(crate) verdict: ReviewVerdict,
    pub(crate) summary: String,
    pub(crate) design_references: Vec<ReviewDesignReference>,
    pub(crate) findings: Vec<ReviewFinding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeliveryScope {
    pub(crate) run: TaskRun,
    pub(crate) work_unit: WorkUnit,
}

#[cfg(test)]
mod tests {
    use pl_protocol::{ProviderFailureKind, RetryDisposition, TurnFailure, TurnFailureCategory};

    use super::TaskFailureDisposition;

    #[test]
    fn task_failure_disposition_uses_typed_failure_semantics() {
        let cases = [
            (
                failure(
                    TurnFailureCategory::Provider,
                    Some(ProviderFailureKind::Authentication),
                    RetryDisposition::Permanent,
                ),
                TaskFailureDisposition::Fatal,
            ),
            (
                failure(
                    TurnFailureCategory::Provider,
                    Some(ProviderFailureKind::Configuration),
                    RetryDisposition::Permanent,
                ),
                TaskFailureDisposition::Fatal,
            ),
            (
                failure(
                    TurnFailureCategory::Provider,
                    Some(ProviderFailureKind::Protocol),
                    RetryDisposition::Permanent,
                ),
                TaskFailureDisposition::Fatal,
            ),
            (
                failure(
                    TurnFailureCategory::Provider,
                    Some(ProviderFailureKind::Capacity),
                    RetryDisposition::Permanent,
                ),
                TaskFailureDisposition::Recoverable,
            ),
            (
                failure(
                    TurnFailureCategory::Provider,
                    Some(ProviderFailureKind::Transport),
                    RetryDisposition::Retryable {
                        retry_after_ms: None,
                    },
                ),
                TaskFailureDisposition::Recoverable,
            ),
            (
                failure(
                    TurnFailureCategory::Validation,
                    None,
                    RetryDisposition::Permanent,
                ),
                TaskFailureDisposition::Recoverable,
            ),
            (
                failure(TurnFailureCategory::Tool, None, RetryDisposition::Permanent),
                TaskFailureDisposition::Fatal,
            ),
            (
                failure(
                    TurnFailureCategory::Internal,
                    None,
                    RetryDisposition::Permanent,
                ),
                TaskFailureDisposition::Fatal,
            ),
        ];

        for (failure, expected) in cases {
            assert_eq!(
                TaskFailureDisposition::for_turn_failure(&failure),
                expected,
                "unexpected disposition for {failure:?}"
            );
        }
    }

    fn failure(
        category: TurnFailureCategory,
        provider_kind: Option<ProviderFailureKind>,
        retry: RetryDisposition,
    ) -> TurnFailure {
        TurnFailure {
            category,
            provider_kind,
            code: None,
            http_status: None,
            message: "failure".to_string(),
            retry,
        }
    }
}
