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

pub(crate) const REVIEW_FILE_COVERAGE_VERSION: u32 = 1;

/// ReviewRound 创建时冻结的文件审查状态及最近一次提交诊断。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReviewFileCoverage {
    pub(crate) version: u32,
    pub(crate) diagnostics_revision: u64,
    pub(crate) files: Vec<ReviewFileReview>,
    pub(crate) last_diagnostics: Option<ReviewExitDiagnostics>,
}

impl ReviewFileCoverage {
    pub(crate) fn pending(mut paths: Vec<String>) -> Self {
        paths.sort();
        paths.dedup();
        Self {
            version: REVIEW_FILE_COVERAGE_VERSION,
            diagnostics_revision: 0,
            files: paths
                .into_iter()
                .map(|path| ReviewFileReview {
                    path,
                    reviewed: false,
                })
                .collect(),
            last_diagnostics: None,
        }
    }

    pub(crate) fn expected_paths(&self) -> Vec<String> {
        self.files.iter().map(|file| file.path.clone()).collect()
    }

    pub(crate) fn reviewed_count(&self) -> usize {
        self.files.iter().filter(|file| file.reviewed).count()
    }

    pub(crate) fn is_complete(&self) -> bool {
        self.files.iter().all(|file| file.reviewed)
    }

    #[cfg(test)]
    pub(crate) fn accepted_attempt(&self) -> Self {
        Self {
            version: self.version,
            diagnostics_revision: self.diagnostics_revision,
            files: self
                .files
                .iter()
                .map(|file| ReviewFileReview {
                    path: file.path.clone(),
                    reviewed: true,
                })
                .collect(),
            last_diagnostics: Some(ReviewExitDiagnostics {
                submitted_count: self.files.len(),
                missing_files: Vec::new(),
                unreviewed_files: Vec::new(),
                duplicate_files: Vec::new(),
                extra_files: Vec::new(),
                invalid_paths: Vec::new(),
                violations: Vec::new(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReviewFileReview {
    pub(crate) path: String,
    pub(crate) reviewed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReviewExitDiagnostics {
    pub(crate) submitted_count: usize,
    pub(crate) missing_files: Vec<String>,
    pub(crate) unreviewed_files: Vec<String>,
    pub(crate) duplicate_files: Vec<String>,
    pub(crate) extra_files: Vec<String>,
    pub(crate) invalid_paths: Vec<ReviewInvalidPath>,
    pub(crate) violations: Vec<ReviewExitViolation>,
}

impl ReviewExitDiagnostics {
    pub(crate) fn is_empty(&self) -> bool {
        self.missing_files.is_empty()
            && self.unreviewed_files.is_empty()
            && self.duplicate_files.is_empty()
            && self.extra_files.is_empty()
            && self.invalid_paths.is_empty()
            && self.violations.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReviewInvalidPath {
    pub(crate) path: String,
    pub(crate) reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReviewExitViolation {
    pub(crate) code: String,
    pub(crate) message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) location: Option<String>,
}

pub(crate) struct BeginIntegratedReview {
    pub(crate) requested_by_call_id: String,
    pub(crate) reviewed_head: String,
    pub(crate) changed_files: Vec<String>,
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
