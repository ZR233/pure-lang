use std::path::PathBuf;

use anyhow::Result;

use super::{
    AgentOutcomeRecord, TaskCoordinator, TaskRunPhase, TaskWorktreeCreationState,
    TaskWorktreeOwnerSnapshot, WorkUnitStatus,
};
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
                let disposition = if terminal_cleanup || unit.status == WorkUnitStatus::Merged {
                    DurableWorktreeDisposition::Cleanup
                } else {
                    DurableWorktreeDisposition::Protect
                };
                resources.push(DurableWorktreeResource {
                    task_run_id: owner.run.id.clone(),
                    path: unit.worktree_path.clone().into(),
                    branch: unit.branch.clone(),
                    expected_head: protected_expected_head(disposition, outcome),
                    presence: match resource.creation_state {
                        TaskWorktreeCreationState::MustExist => DurableWorktreePresence::MustExist,
                        TaskWorktreeCreationState::UncreatedBeforeRestart => {
                            DurableWorktreePresence::MayBeUncreated
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
