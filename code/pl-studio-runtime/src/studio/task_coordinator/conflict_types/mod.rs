use serde::{Deserialize, Serialize};

use super::{ConflictKind, MergeIndexStage, MergeVerificationStep};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConflictVerificationEvidence {
    pub(crate) attempt: u32,
    pub(crate) success: bool,
    pub(crate) index_tree: Option<String>,
    pub(crate) steps: Vec<MergeVerificationStep>,
    pub(crate) diagnostic: Option<String>,
}

pub(crate) struct RecordConflictVerification {
    pub(crate) merge_id: String,
    pub(crate) expected_head: String,
    pub(crate) success: bool,
    pub(crate) index_tree: Option<String>,
    pub(crate) steps: Vec<MergeVerificationStep>,
    pub(crate) diagnostic: Option<String>,
}

pub(crate) struct CompleteConflictMerge {
    pub(crate) merge_id: String,
    pub(crate) expected_head: String,
    pub(crate) merge_commit: String,
    pub(crate) resolution_summary: String,
}

pub(crate) struct AbortConflictMerge {
    pub(crate) merge_id: String,
    pub(crate) expected_head: String,
    pub(crate) reason: String,
    pub(crate) compensation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConflictVerificationOutput {
    pub(crate) merge_id: String,
    pub(crate) attempt: u32,
    pub(crate) success: bool,
    pub(crate) diagnostics: Vec<String>,
    pub(crate) verification: Vec<MergeVerificationStep>,
    pub(crate) aborted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConflictListItem {
    pub(crate) path: String,
    pub(crate) kind: ConflictKind,
    pub(crate) stages: Vec<MergeIndexStage>,
    pub(crate) resolved: bool,
    pub(crate) binary: bool,
    pub(crate) rename_source: Option<String>,
    pub(crate) rename_destination: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConflictBlob {
    pub(crate) available: bool,
    pub(crate) binary: bool,
    pub(crate) content: Option<String>,
    pub(crate) object_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConflictReadOutput {
    pub(crate) merge_id: String,
    pub(crate) path: String,
    pub(crate) kind: ConflictKind,
    pub(crate) binary: bool,
    pub(crate) base: ConflictBlob,
    pub(crate) ours: ConflictBlob,
    pub(crate) theirs: ConflictBlob,
    pub(crate) combined_diff: String,
    pub(crate) design_references: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConflictResolveOutput {
    pub(crate) merge_id: String,
    pub(crate) path: String,
    pub(crate) strategy: String,
    pub(crate) unresolved_paths: Vec<String>,
}
