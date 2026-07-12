use anyhow::{Context, Result};

use super::accept::pending_cleanup;
use crate::studio::task_coordinator::{
    MergeRecord, MergeStatus, TaskMergeAgentOutput, TaskMergeScope,
};

pub(super) fn empty_preflight_merge() -> MergeRecord {
    MergeRecord {
        id: String::new(),
        task_run_id: String::new(),
        agent_id: String::new(),
        status: MergeStatus::Pending,
        expected_head: String::new(),
        source_commit: String::new(),
        conflict_files: Vec::new(),
        resolution_summary: None,
        verification: None,
        evidence: None,
        attempt: 0,
        created_at: 0,
        updated_at: 0,
    }
}

pub(super) fn merged_output(scope: &TaskMergeScope) -> Result<TaskMergeAgentOutput> {
    let evidence = scope
        .merge
        .evidence
        .as_ref()
        .context("accepted merge has no versioned evidence")?;
    let merge_commit = evidence
        .merge_commit
        .clone()
        .context("accepted merge has no merge commit")?;
    Ok(TaskMergeAgentOutput {
        merge_id: scope.merge.id.clone(),
        status: MergeStatus::Merged,
        previous_head: scope.merge.expected_head.clone(),
        new_head: Some(merge_commit),
        agent_id: scope.outcome.agent_id.clone(),
        source_commit: scope.delivery.head_commit.clone(),
        changed_files: evidence.changed_files.clone(),
        verification: evidence.verification_steps.clone(),
        cleanup: evidence.cleanup.clone().unwrap_or_else(pending_cleanup),
        conflict_files: Vec::new(),
    })
}
