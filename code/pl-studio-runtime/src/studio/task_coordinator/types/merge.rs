use super::*;
use schemars::JsonSchema;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MergeRecord {
    pub(crate) id: String,
    pub(crate) task_run_id: String,
    pub(crate) work_unit_id: String,
    pub(crate) completion_id: String,
    pub(crate) completion_revision: u32,
    pub(crate) executor_agent_id: String,
    pub(crate) expected_previous_head: String,
    pub(crate) resulting_head: String,
    pub(crate) delivery_head: String,
    pub(crate) method: MergeMethod,
    pub(crate) summary: String,
    pub(crate) cleanup: MergeCleanupEvidence,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) enum MergeMethod {
    Merge,
    CherryPick,
    Squash,
    Rebase,
    Manual,
}

impl MergeMethod {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Merge => "merge",
            Self::CherryPick => "cherryPick",
            Self::Squash => "squash",
            Self::Rebase => "rebase",
            Self::Manual => "manual",
        }
    }

    pub(crate) fn from_str(value: &str) -> Option<Self> {
        match value {
            "merge" => Some(Self::Merge),
            "cherryPick" => Some(Self::CherryPick),
            "squash" => Some(Self::Squash),
            "rebase" => Some(Self::Rebase),
            "manual" => Some(Self::Manual),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MergeCandidate {
    pub(crate) executor_agent_id: String,
    pub(crate) completion_revision: u32,
    pub(crate) relative_worktree_path: String,
    pub(crate) branch: String,
    pub(crate) base_commit: String,
    pub(crate) head_commit: String,
    pub(crate) expected_task_head: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MergeCleanupEvidence {
    pub(crate) status: String,
    pub(crate) detail: Option<String>,
}

pub(crate) struct TaskMergeScope {
    pub(crate) run: TaskRunRecord,
    pub(crate) work_unit: WorkUnitRecord,
    pub(crate) completion: WorkCompletionRecord,
    pub(crate) delivery: AgentDelivery,
    pub(crate) merge: MergeRecord,
}

pub(crate) struct RecordTaskMerge {
    pub(crate) thread_id: String,
    pub(crate) executor_agent_id: String,
    pub(crate) work_unit_id: String,
    pub(crate) completion_id: String,
    pub(crate) completion_revision: u32,
    pub(crate) expected_previous_head: String,
    pub(crate) resulting_head: String,
    pub(crate) method: MergeMethod,
    pub(crate) summary: String,
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
    pub(crate) reviewer_thread_id: Option<String>,
    pub(crate) reviewer_status: ThreadExecutionStatus,
    pub(crate) reviewer_error: Option<String>,
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
    pub(crate) scope_hints: Vec<String>,
    pub(crate) base_commit: String,
    pub(crate) worktree_path: String,
    pub(crate) branch: String,
    pub(crate) attempt: u32,
}

pub(crate) struct AllocateExecutor {
    pub(crate) thread_id: String,
    pub(crate) title: String,
    pub(crate) scope_hints: Vec<String>,
    pub(crate) agent_id: String,
    pub(crate) requested_by_call_id: String,
}

pub(crate) struct ExecutorAllocation {
    pub(crate) run: TaskRunRecord,
    pub(crate) work_unit: WorkUnitRecord,
    pub(crate) reused: bool,
}
