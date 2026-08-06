use std::path::PathBuf;

use anyhow::{Result, bail};

use super::{
    TaskCoordinator, TaskWorktreeCleanupState, TaskWorktreeCreationState, TaskWorktreeDisposition,
    TaskWorktreeOwnerSnapshot, WorkCompletionRecord,
};
use crate::agent::worktree::{
    DurableWorktreeDisposition, DurableWorktreePresence, DurableWorktreeResource,
    WorktreeReconciliation, reconcile_task_worktree_group,
};

mod merge;
#[cfg(test)]
pub(crate) use merge::MERGE_RECOVERY_BLOCK_PREFIX;
pub(crate) use merge::{
    MergingRecovery, inspect_merging_recovery, is_retryable_merge_recovery_message,
    validate_snapshot_owner,
};

impl TaskCoordinator {
    pub(super) async fn reconcile_durable_worktrees(
        &self,
        repositories: &[PathBuf],
        owners: &[TaskWorktreeOwnerSnapshot],
    ) -> Result<()> {
        let mut resources = Vec::new();
        for owner in owners {
            for resource in &owner.resources {
                let unit = &resource.work_unit;
                let disposition = if unit.worktree_disposition
                    == TaskWorktreeDisposition::CleanupRequested
                {
                    DurableWorktreeDisposition::Cleanup
                } else {
                    match &resource.cleanup_state {
                        TaskWorktreeCleanupState::Cleanup => DurableWorktreeDisposition::Cleanup,
                        TaskWorktreeCleanupState::Replay { merge_id } => {
                            self.replay_accepted_cleanup(merge_id).await?;
                            DurableWorktreeDisposition::Cleanup
                        }
                        TaskWorktreeCleanupState::Protect => DurableWorktreeDisposition::Protect,
                        TaskWorktreeCleanupState::NotMerged => DurableWorktreeDisposition::Protect,
                    }
                };
                resources.push(DurableWorktreeResource {
                    task_run_id: owner.run.id.clone(),
                    path: unit.worktree_path.clone().into(),
                    branch: unit.branch.clone(),
                    expected_head: protected_expected_head(
                        disposition,
                        resource.completion.as_ref(),
                    ),
                    presence: if disposition == DurableWorktreeDisposition::Cleanup {
                        DurableWorktreePresence::MayBeUncreated
                    } else {
                        match resource.creation_state {
                            TaskWorktreeCreationState::MustExist => {
                                DurableWorktreePresence::MustExist
                            }
                            TaskWorktreeCreationState::UncreatedBeforeRestart => {
                                DurableWorktreePresence::MayBeUncreated
                            }
                        }
                    },
                    disposition,
                });
            }
        }
        let _summary: WorktreeReconciliation =
            reconcile_task_worktree_group(repositories, &resources).await?;
        Ok(())
    }

    async fn replay_accepted_cleanup(&self, merge_id: &str) -> Result<()> {
        let scope = self.store.read_accepted_merge_scope(merge_id).await?;
        self.validate_accepted_cleanup_replay(&scope).await?;
        self.store.record_merge_cleanup_attempting(merge_id).await?;
        let cleanup = super::merge::cleanup_accepted_delivery(&scope, None).await;
        self.store
            .record_merge_cleanup(merge_id, cleanup.clone())
            .await?;
        if cleanup.status == "failed" {
            bail!(
                "accepted cleanup replay failed: {}",
                cleanup
                    .detail
                    .as_deref()
                    .unwrap_or("unknown cleanup failure")
            );
        }
        Ok(())
    }
}

fn protected_expected_head(
    disposition: DurableWorktreeDisposition,
    completion: Option<&WorkCompletionRecord>,
) -> Option<String> {
    if disposition == DurableWorktreeDisposition::Cleanup {
        return None;
    }
    completion.and_then(|completion| completion.head_commit.clone())
}
