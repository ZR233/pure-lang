use anyhow::{Result, bail};

use super::super::TaskRunRecord;
use super::super::git::{RepositorySnapshot, ensure_no_git_operation, inspect_repository};
use crate::agent::worktree::same_worktree_path;

pub(crate) const MERGE_RECOVERY_BLOCK_PREFIX: &str =
    "merge recovery requires planner reconciliation:";
const LEGACY_MERGE_RECOVERY_BLOCK_MESSAGE: &str = "planner Git integration was interrupted before task_record_merge; preserving the workspace for manual accounting";

pub(crate) enum MergingRecovery {
    Resume,
    Retry(String),
}

pub(crate) fn is_retryable_merge_recovery_message(message: &str) -> bool {
    message.starts_with(MERGE_RECOVERY_BLOCK_PREFIX)
        || message == LEGACY_MERGE_RECOVERY_BLOCK_MESSAGE
}

pub(crate) fn validate_snapshot_owner(
    run: &TaskRunRecord,
    snapshot: &RepositorySnapshot,
) -> Result<()> {
    if !same_worktree_path(&run.workspace_root, &snapshot.workspace_root) {
        bail!("task workspace changed outside the coordinator");
    }
    if !same_worktree_path(&run.git_common_dir, &snapshot.git_common_dir)
        || run.branch != snapshot.branch
    {
        bail!("task branch changed outside the coordinator");
    }
    Ok(())
}

pub(crate) async fn inspect_merging_recovery(run: &TaskRunRecord) -> MergingRecovery {
    let snapshot = match inspect_repository(&run.workspace_root, false).await {
        Ok(snapshot) => snapshot,
        Err(error) => {
            return MergingRecovery::Retry(format!(
                "{MERGE_RECOVERY_BLOCK_PREFIX} repository inspection failed: {error}"
            ));
        }
    };
    if let Err(error) = validate_snapshot_owner(run, &snapshot) {
        return MergingRecovery::Retry(format!("{MERGE_RECOVERY_BLOCK_PREFIX} {error}"));
    }

    let mut reasons = Vec::new();
    if snapshot.head != run.expected_head {
        reasons.push(format!(
            "HEAD changed from {} to {} before task_record_merge",
            run.expected_head, snapshot.head
        ));
    }
    if let Err(error) = inspect_repository(&run.workspace_root, true).await {
        reasons.push(error.to_string());
    }
    if let Err(error) = ensure_no_git_operation(&run.workspace_root).await {
        reasons.push(error.to_string());
    }
    if reasons.is_empty() {
        MergingRecovery::Resume
    } else {
        MergingRecovery::Retry(format!(
            "{MERGE_RECOVERY_BLOCK_PREFIX} {}",
            reasons.join("; ")
        ))
    }
}
