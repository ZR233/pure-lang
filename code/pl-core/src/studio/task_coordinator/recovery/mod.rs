use anyhow::Result;

use super::{AgentOutcomeRecord, TaskCoordinator, TaskRunPhase, TaskRunRecord, WorkUnitStatus};
use crate::agent::worktree::{
    DurableWorktreeDisposition, DurableWorktreeResource, WorktreeReconciliation,
    reconcile_task_worktrees,
};

impl TaskCoordinator {
    pub(super) async fn reconcile_durable_worktrees(&self, run: &TaskRunRecord) -> Result<()> {
        let owners = self
            .store
            .list_task_worktree_owners(&run.workspace_root)
            .await?;
        let mut resources = Vec::new();
        for owner in owners {
            let terminal_cleanup = matches!(
                owner.run.phase,
                TaskRunPhase::Completed | TaskRunPhase::Failed | TaskRunPhase::Cancelled
            );
            for unit in owner.work_units {
                let outcome = owner.outcomes.iter().find(|outcome| {
                    outcome.work_unit_id.as_deref() == Some(unit.id.as_str())
                        && outcome.agent_id == unit.agent_id.as_deref().unwrap_or_default()
                });
                let disposition = if terminal_cleanup || unit.status == WorkUnitStatus::Merged {
                    DurableWorktreeDisposition::Cleanup
                } else {
                    DurableWorktreeDisposition::Protect
                };
                resources.push(DurableWorktreeResource {
                    task_run_id: owner.run.id.clone(),
                    path: unit.worktree_path.into(),
                    branch: unit.branch,
                    expected_head: protected_expected_head(disposition, outcome),
                    disposition,
                });
            }
        }
        let _summary: WorktreeReconciliation =
            reconcile_task_worktrees(&run.workspace_root, &resources).await?;
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
