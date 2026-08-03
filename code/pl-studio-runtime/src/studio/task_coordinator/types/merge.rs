use super::*;

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
    pub(crate) completion_id: String,
    pub(crate) completion_revision: u32,
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
    pub(crate) run: TaskRunRecord,
    pub(crate) lease: BranchLeaseRecord,
    pub(crate) work_unit: WorkUnitRecord,
    pub(crate) outcome: AgentOutcomeRecord,
    pub(crate) completion: WorkCompletionRecord,
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
    pub(crate) scope: ReviewScope,
    pub(crate) work_unit_id: Option<String>,
    pub(crate) completion_id: Option<String>,
    pub(crate) completion_revision: Option<u32>,
    pub(crate) reviewed_head: String,
    pub(crate) verdict: ReviewVerdict,
    pub(crate) requested_by_call_id: String,
    pub(crate) reviewer_agent_id: Option<String>,
    pub(crate) summary: Option<String>,
    pub(crate) design_references: Vec<ReviewDesignReference>,
    pub(crate) findings: Vec<ReviewFinding>,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ReviewScope {
    Delivery,
    Integrated,
}

impl ReviewScope {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Delivery => "delivery",
            Self::Integrated => "integrated",
        }
    }

    pub(crate) fn from_str(value: &str) -> Option<Self> {
        match value {
            "delivery" => Some(Self::Delivery),
            "integrated" => Some(Self::Integrated),
            _ => None,
        }
    }
}

#[cfg(test)]
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

#[cfg(test)]
pub(crate) struct CreateMergeRecord {
    pub(crate) task_run_id: String,
    pub(crate) agent_id: String,
    pub(crate) expected_head: String,
    pub(crate) source_commit: String,
    pub(crate) conflict_files: Vec<String>,
}

#[cfg(test)]
pub(crate) struct UpdateAgentOutcome {
    pub(crate) status: AgentOutcomeStatus,
    pub(crate) summary: Option<String>,
    pub(crate) error: Option<String>,
}

#[cfg(test)]
pub(crate) struct UpdateMergeRecord {
    pub(crate) status: MergeStatus,
    pub(crate) resolution_summary: Option<String>,
    pub(crate) verification: Option<Vec<String>>,
    pub(crate) attempt: u32,
}
