use serde::{Deserialize, Serialize};

use super::conflict_types::ConflictVerificationEvidence;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum TaskRunPhase {
    Planning,
    PendingConfirmation,
    DesignUpdating,
    Implementing,
    Merging,
    ResolvingConflict,
    Reviewing,
    Reworking,
    Completed,
    Blocked,
    Failed,
    Cancelled,
}

impl TaskRunPhase {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Planning => "planning",
            Self::PendingConfirmation => "pendingConfirmation",
            Self::DesignUpdating => "designUpdating",
            Self::Implementing => "implementing",
            Self::Merging => "merging",
            Self::ResolvingConflict => "resolvingConflict",
            Self::Reviewing => "reviewing",
            Self::Reworking => "reworking",
            Self::Completed => "completed",
            Self::Blocked => "blocked",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub(crate) fn from_str(value: &str) -> Option<Self> {
        match value {
            "planning" => Some(Self::Planning),
            "pendingConfirmation" => Some(Self::PendingConfirmation),
            "designUpdating" => Some(Self::DesignUpdating),
            "implementing" => Some(Self::Implementing),
            "merging" => Some(Self::Merging),
            "resolvingConflict" => Some(Self::ResolvingConflict),
            "reviewing" => Some(Self::Reviewing),
            "reworking" => Some(Self::Reworking),
            "completed" => Some(Self::Completed),
            "blocked" => Some(Self::Blocked),
            "failed" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }

    pub(crate) fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Blocked | Self::Failed | Self::Cancelled
        )
    }

    pub(crate) fn can_transition_to(self, next: Self) -> bool {
        if self == next {
            return true;
        }
        if next == Self::Cancelled {
            return !matches!(self, Self::Completed | Self::Failed | Self::Cancelled);
        }
        if next == Self::Blocked {
            return !self.is_terminal();
        }
        match self {
            Self::Planning => matches!(next, Self::PendingConfirmation | Self::Failed),
            Self::PendingConfirmation => {
                matches!(next, Self::DesignUpdating | Self::Planning | Self::Failed)
            }
            Self::DesignUpdating => matches!(next, Self::Implementing | Self::Failed),
            Self::Implementing => matches!(
                next,
                Self::Merging | Self::Reviewing | Self::Blocked | Self::Failed
            ),
            Self::Merging => matches!(
                next,
                Self::Implementing
                    | Self::ResolvingConflict
                    | Self::Reviewing
                    | Self::Blocked
                    | Self::Failed
            ),
            Self::ResolvingConflict => {
                matches!(next, Self::Merging | Self::Blocked | Self::Failed)
            }
            Self::Reviewing => {
                matches!(
                    next,
                    Self::Reworking | Self::Completed | Self::Blocked | Self::Failed
                )
            }
            Self::Reworking => matches!(next, Self::Implementing | Self::Blocked | Self::Failed),
            Self::Completed | Self::Blocked | Self::Failed | Self::Cancelled => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskRunRecord {
    pub(crate) id: String,
    pub(crate) session_id: String,
    pub(crate) phase: TaskRunPhase,
    pub(crate) plan: String,
    pub(crate) workspace_root: String,
    pub(crate) git_common_dir: String,
    pub(crate) branch: String,
    pub(crate) base_commit: String,
    pub(crate) expected_head: String,
    pub(crate) design_commit: Option<String>,
    pub(crate) status_message: Option<String>,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
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
    pub(crate) cancelled_outcomes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TaskWorktreeOwnerSnapshot {
    pub(crate) run: TaskRunRecord,
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
    pub(crate) work_unit: WorkUnitRecord,
    pub(crate) outcome: Option<AgentOutcomeRecord>,
    pub(crate) creation_state: TaskWorktreeCreationState,
    pub(crate) cleanup_state: TaskWorktreeCleanupState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DesignUpdateOutput {
    pub(crate) task_run_id: String,
    pub(crate) previous_head: String,
    pub(crate) design_commit: String,
    pub(crate) changed_files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DesignCancellationRevert {
    pub(crate) task_run_id: String,
    pub(crate) previous_head: String,
    pub(crate) revert_commit: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CreateTaskRun {
    pub(crate) session_id: String,
    pub(crate) phase: TaskRunPhase,
    pub(crate) plan: String,
    pub(crate) workspace_root: String,
    pub(crate) git_common_dir: String,
    pub(crate) branch: String,
    pub(crate) head_commit: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum WorkUnitStatus {
    Pending,
    Running,
    WaitingForDelivery,
    Delivered,
    Merged,
    Failed,
    Cancelled,
}

impl WorkUnitStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::WaitingForDelivery => "waitingForDelivery",
            Self::Delivered => "delivered",
            Self::Merged => "merged",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub(crate) fn from_str(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "running" => Some(Self::Running),
            "waitingForDelivery" => Some(Self::WaitingForDelivery),
            "delivered" => Some(Self::Delivered),
            "merged" => Some(Self::Merged),
            "failed" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum AgentOutcomeStatus {
    Queued,
    Running,
    WaitingForDelivery,
    Completed,
    Failed,
    Cancelled,
}

impl AgentOutcomeStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::WaitingForDelivery => "waitingForDelivery",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub(crate) fn from_str(value: &str) -> Option<Self> {
        match value {
            "queued" => Some(Self::Queued),
            "running" => Some(Self::Running),
            "waitingForDelivery" => Some(Self::WaitingForDelivery),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum MergeStatus {
    Pending,
    Conflicted,
    Verifying,
    Merged,
    Aborted,
    Failed,
}

impl MergeStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Conflicted => "conflicted",
            Self::Verifying => "verifying",
            Self::Merged => "merged",
            Self::Aborted => "aborted",
            Self::Failed => "failed",
        }
    }

    pub(crate) fn from_str(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "conflicted" => Some(Self::Conflicted),
            "verifying" => Some(Self::Verifying),
            "merged" => Some(Self::Merged),
            "aborted" => Some(Self::Aborted),
            "failed" => Some(Self::Failed),
            _ => None,
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

    pub(crate) fn from_str(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "pass" => Some(Self::Pass),
            "changesRequired" => Some(Self::ChangesRequired),
            "blocked" => Some(Self::Blocked),
            "failed" => Some(Self::Failed),
            _ => None,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReviewDesignReference {
    pub(crate) path: String,
    pub(crate) section: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReviewFinding {
    pub(crate) severity: String,
    pub(crate) title: String,
    pub(crate) body: String,
    pub(crate) path: Option<String>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkUnitRecord {
    pub(crate) id: String,
    pub(crate) task_run_id: String,
    pub(crate) title: String,
    pub(crate) status: WorkUnitStatus,
    pub(crate) owned_paths: Vec<String>,
    pub(crate) base_commit: String,
    pub(crate) worktree_path: String,
    pub(crate) branch: String,
    pub(crate) attempt: u32,
    pub(crate) agent_id: Option<String>,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentOutcomeRecord {
    pub(crate) id: String,
    pub(crate) task_run_id: String,
    pub(crate) work_unit_id: Option<String>,
    pub(crate) agent_id: String,
    pub(crate) owner_path: String,
    pub(crate) initiated_by: String,
    pub(crate) requested_by_call_id: String,
    pub(crate) role: String,
    pub(crate) status: AgentOutcomeStatus,
    pub(crate) attempt: u32,
    pub(crate) summary: Option<String>,
    pub(crate) error: Option<String>,
    pub(crate) delivery: Option<AgentDelivery>,
    pub(crate) review: Option<AgentReview>,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeliveryScope {
    pub(crate) run: TaskRunRecord,
    pub(crate) work_unit: WorkUnitRecord,
    pub(crate) outcome: AgentOutcomeRecord,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DeliveryScopeResolution {
    Resolved(DeliveryScope),
    MissingWorkUnit(AgentOutcomeRecord),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MergeRecord {
    pub(crate) id: String,
    pub(crate) task_run_id: String,
    pub(crate) agent_id: String,
    pub(crate) status: MergeStatus,
    pub(crate) expected_head: String,
    pub(crate) source_commit: String,
    pub(crate) conflict_files: Vec<String>,
    pub(crate) resolution_summary: Option<String>,
    pub(crate) verification: Option<Vec<String>>,
    pub(crate) evidence: Option<MergeEvidence>,
    pub(crate) attempt: u32,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MergeEvidence {
    pub(crate) version: u32,
    pub(crate) origin_phase: TaskRunPhase,
    pub(crate) work_unit_id: String,
    pub(crate) outcome_id: String,
    pub(crate) delivery_head: String,
    pub(crate) pre_index_tree: String,
    pub(crate) changed_files: Vec<String>,
    #[serde(default)]
    pub(crate) verification_steps: Vec<MergeVerificationStep>,
    #[serde(default)]
    pub(crate) merge_commit: Option<String>,
    #[serde(default)]
    pub(crate) conflict_manifest: Option<ConflictManifest>,
    #[serde(default)]
    pub(crate) conflict_continuation_requested: bool,
    #[serde(default)]
    pub(crate) merge_completion_continuation_requested: bool,
    #[serde(default)]
    pub(crate) conflict_verification: Option<ConflictVerificationEvidence>,
    #[serde(default)]
    pub(crate) compensation: Option<String>,
    #[serde(default)]
    pub(crate) cleanup: Option<MergeCleanupEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MergeVerificationStep {
    pub(crate) command: Vec<String>,
    pub(crate) success: bool,
    pub(crate) output: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MergeCleanupEvidence {
    pub(crate) status: String,
    pub(crate) detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConflictManifest {
    pub(crate) merge_head: String,
    pub(crate) merge_base: String,
    pub(crate) pre_index_tree: String,
    pub(crate) conflicts: Vec<ConflictEntry>,
    #[serde(default)]
    pub(crate) status_porcelain_v1_z: Vec<u8>,
    #[serde(default)]
    pub(crate) index_stage_zero_entries: Vec<MergeIndexEntry>,
    pub(crate) auto_merged_entries: Vec<MergeIndexEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConflictEntry {
    pub(crate) path: String,
    pub(crate) kind: ConflictKind,
    pub(crate) stages: Vec<MergeIndexStage>,
    #[serde(default)]
    pub(crate) worktree_object_id: Option<String>,
    pub(crate) binary: bool,
    pub(crate) rename_source: Option<String>,
    pub(crate) rename_destination: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ConflictKind {
    Text,
    AddAdd,
    RenameDelete,
    ModifyDelete,
    Binary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MergeIndexStage {
    pub(crate) stage: u8,
    pub(crate) mode: String,
    pub(crate) object_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MergeIndexEntry {
    pub(crate) path: String,
    pub(crate) mode: String,
    pub(crate) object_id: String,
}

pub(crate) struct BeginTaskMerge {
    pub(crate) session_id: String,
    pub(crate) agent_id: String,
    pub(crate) expected_head: String,
    pub(crate) pre_index_tree: String,
    pub(crate) changed_files: Vec<String>,
}

pub(crate) struct TaskMergeScope {
    #[cfg(test)]
    pub(crate) origin_phase: TaskRunPhase,
    pub(crate) run: TaskRunRecord,
    pub(crate) lease: BranchLeaseRecord,
    pub(crate) work_unit: WorkUnitRecord,
    pub(crate) outcome: AgentOutcomeRecord,
    pub(crate) delivery: AgentDelivery,
    pub(crate) merge: MergeRecord,
}

#[derive(Debug, Clone)]
pub(crate) struct MergeVerificationRequest {
    pub(crate) workspace_root: String,
    pub(crate) changed_files: Vec<String>,
}

pub(crate) struct CompleteTaskMerge {
    pub(crate) merge_id: String,
    pub(crate) expected_head: String,
    pub(crate) merge_commit: String,
    pub(crate) verification_steps: Vec<MergeVerificationStep>,
}

pub(crate) struct FailTaskMerge {
    pub(crate) merge_id: String,
    pub(crate) reason: String,
    pub(crate) verification_steps: Vec<MergeVerificationStep>,
    pub(crate) compensation: Option<String>,
}

pub(crate) struct ConflictTaskMerge {
    pub(crate) merge_id: String,
    pub(crate) manifest: ConflictManifest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskMergeAgentOutput {
    pub(crate) merge_id: String,
    pub(crate) status: MergeStatus,
    pub(crate) previous_head: String,
    pub(crate) new_head: Option<String>,
    pub(crate) agent_id: String,
    pub(crate) source_commit: String,
    pub(crate) changed_files: Vec<String>,
    pub(crate) verification: Vec<MergeVerificationStep>,
    pub(crate) cleanup: MergeCleanupEvidence,
    pub(crate) conflict_files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReviewRoundRecord {
    pub(crate) id: String,
    pub(crate) task_run_id: String,
    pub(crate) round: u32,
    pub(crate) head_commit: String,
    pub(crate) verdict: ReviewVerdict,
    pub(crate) reviewer_agent_id: Option<String>,
    pub(crate) summary: Option<String>,
    pub(crate) design_references: Vec<ReviewDesignReference>,
    pub(crate) findings: Vec<ReviewFinding>,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
}

pub(crate) struct CreateWorkUnit {
    pub(crate) task_run_id: String,
    pub(crate) title: String,
    pub(crate) owned_paths: Vec<String>,
    pub(crate) base_commit: String,
    pub(crate) worktree_path: String,
    pub(crate) branch: String,
    pub(crate) attempt: u32,
}

pub(crate) struct CreateAgentOutcome {
    pub(crate) task_run_id: String,
    pub(crate) work_unit_id: Option<String>,
    pub(crate) agent_id: String,
    pub(crate) owner_path: String,
    pub(crate) initiated_by: String,
    pub(crate) requested_by_call_id: String,
    pub(crate) role: String,
    pub(crate) status: AgentOutcomeStatus,
    pub(crate) attempt: u32,
}

pub(crate) struct AllocateExecutor {
    pub(crate) session_id: String,
    pub(crate) title: String,
    pub(crate) owned_paths: Vec<String>,
    pub(crate) agent_id: String,
    pub(crate) owner_path: String,
    pub(crate) requested_by_call_id: String,
}

pub(crate) struct ExecutorAllocation {
    pub(crate) run: TaskRunRecord,
    pub(crate) work_unit: WorkUnitRecord,
    pub(crate) outcome: AgentOutcomeRecord,
}

pub(crate) struct CreateMergeRecord {
    pub(crate) task_run_id: String,
    pub(crate) agent_id: String,
    pub(crate) expected_head: String,
    pub(crate) source_commit: String,
    pub(crate) conflict_files: Vec<String>,
}

pub(crate) struct UpdateAgentOutcome {
    pub(crate) status: AgentOutcomeStatus,
    pub(crate) summary: Option<String>,
    pub(crate) error: Option<String>,
    pub(crate) delivery: Option<AgentDelivery>,
    pub(crate) review: Option<AgentReview>,
}

pub(crate) struct UpdateMergeRecord {
    pub(crate) status: MergeStatus,
    pub(crate) resolution_summary: Option<String>,
    pub(crate) verification: Option<Vec<String>>,
    pub(crate) attempt: u32,
}

pub(crate) struct CreateReviewRound {
    pub(crate) task_run_id: String,
    pub(crate) round: u32,
    pub(crate) head_commit: String,
    pub(crate) reviewer_agent_id: Option<String>,
}

pub(crate) struct CompleteReviewRound {
    pub(crate) verdict: ReviewVerdict,
    pub(crate) summary: String,
    pub(crate) design_references: Vec<ReviewDesignReference>,
    pub(crate) findings: Vec<ReviewFinding>,
}
