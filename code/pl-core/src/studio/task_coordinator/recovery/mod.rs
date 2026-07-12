use std::path::PathBuf;

use anyhow::{Result, bail};

use super::{
    AgentOutcomeRecord, TaskCoordinator, TaskRunPhase, TaskWorktreeCleanupState,
    TaskWorktreeCreationState, TaskWorktreeOwnerSnapshot,
};
use crate::AgentSupervisor;
use crate::agent::worktree::{
    DurableWorktreeDisposition, DurableWorktreePresence, DurableWorktreeResource,
    WorktreeReconciliation, reconcile_task_worktree_group,
};

impl TaskCoordinator {
    pub(super) async fn reconcile_durable_worktrees(
        &self,
        repositories: &[PathBuf],
        owners: &[TaskWorktreeOwnerSnapshot],
    ) -> Result<()> {
        let mut resources = Vec::new();
        for owner in owners {
            let terminal_cleanup = matches!(
                owner.run.phase,
                TaskRunPhase::Completed | TaskRunPhase::Failed | TaskRunPhase::Cancelled
            );
            for resource in &owner.resources {
                let unit = &resource.work_unit;
                let outcome = resource.outcome.as_ref();
                let disposition = match &resource.cleanup_state {
                    TaskWorktreeCleanupState::Cleanup => DurableWorktreeDisposition::Cleanup,
                    TaskWorktreeCleanupState::Replay { merge_id } => {
                        self.replay_accepted_cleanup(merge_id).await?;
                        DurableWorktreeDisposition::Cleanup
                    }
                    TaskWorktreeCleanupState::Protect => DurableWorktreeDisposition::Protect,
                    TaskWorktreeCleanupState::NotMerged if terminal_cleanup => {
                        DurableWorktreeDisposition::Cleanup
                    }
                    TaskWorktreeCleanupState::NotMerged => DurableWorktreeDisposition::Protect,
                };
                resources.push(DurableWorktreeResource {
                    task_run_id: owner.run.id.clone(),
                    path: unit.worktree_path.clone().into(),
                    branch: unit.branch.clone(),
                    expected_head: protected_expected_head(disposition, outcome),
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
        let supervisor = AgentSupervisor::default();
        let (event_tx, _) = tokio::sync::broadcast::channel(8);
        let cleanup = super::merge::cleanup_accepted_delivery(
            &scope,
            &supervisor,
            &event_tx,
            "restart-cleanup-replay",
        )
        .await;
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
    outcome: Option<&AgentOutcomeRecord>,
) -> Option<String> {
    if disposition == DurableWorktreeDisposition::Cleanup {
        return None;
    }
    outcome
        .and_then(|outcome| outcome.delivery.as_ref())
        .map(|delivery| delivery.head_commit.clone())
}
