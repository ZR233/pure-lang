use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cfg(test)]
use super::TaskIssueDisposition;
use super::work_completion::WorkCompletionRecord;
use super::{TaskRun, WorkUnit};

mod merge;
pub(crate) use merge::*;

#[derive(Debug, Clone)]
pub(crate) struct TaskToolRuntime {
    pub(crate) workspace: pl_core::ToolWorkspace,
    pub(crate) session: pl_core::ToolSessionRuntime,
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
    pub(crate) const fn as_str(self) -> &'static str {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CreateTaskRun {
    pub(crate) project_id: String,
    pub(crate) root_thread_id: String,
    pub(crate) request: String,
    pub(crate) workspace_root: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExecutorCloseDisposition {
    PreserveForMerge,
    Discard,
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

    use super::TaskIssueDisposition;

    #[test]
    fn task_issue_disposition_uses_typed_failure_semantics() {
        let cases = [
            (
                failure(
                    TurnFailureCategory::Provider,
                    Some(ProviderFailureKind::Authentication),
                    RetryDisposition::Permanent,
                ),
                TaskIssueDisposition::Fatal,
            ),
            (
                failure(
                    TurnFailureCategory::Provider,
                    Some(ProviderFailureKind::Configuration),
                    RetryDisposition::Permanent,
                ),
                TaskIssueDisposition::Fatal,
            ),
            (
                failure(
                    TurnFailureCategory::Provider,
                    Some(ProviderFailureKind::Protocol),
                    RetryDisposition::Permanent,
                ),
                TaskIssueDisposition::Fatal,
            ),
            (
                failure(
                    TurnFailureCategory::Provider,
                    Some(ProviderFailureKind::Capacity),
                    RetryDisposition::Permanent,
                ),
                TaskIssueDisposition::Recoverable,
            ),
            (
                failure(
                    TurnFailureCategory::Provider,
                    Some(ProviderFailureKind::Transport),
                    RetryDisposition::Retryable {
                        retry_after_ms: None,
                    },
                ),
                TaskIssueDisposition::Recoverable,
            ),
            (
                failure(
                    TurnFailureCategory::Validation,
                    None,
                    RetryDisposition::Permanent,
                ),
                TaskIssueDisposition::Recoverable,
            ),
            (
                failure(TurnFailureCategory::Tool, None, RetryDisposition::Permanent),
                TaskIssueDisposition::Fatal,
            ),
            (
                failure(
                    TurnFailureCategory::Internal,
                    None,
                    RetryDisposition::Permanent,
                ),
                TaskIssueDisposition::Fatal,
            ),
        ];

        for (failure, expected) in cases {
            assert_eq!(
                TaskIssueDisposition::for_turn_failure(&failure),
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
