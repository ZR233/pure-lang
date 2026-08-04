use anyhow::{Context, Result, bail};

use super::cleanup::cleanup_accepted_delivery;
use super::output::empty_preflight_merge;
use super::validation::ensure_preflight_delivery_identity;
use crate::AgentRuntimeHandle;
use crate::studio::task_coordinator::{
    AgentDelivery, AgentWorktreeDelivery, TaskCoordinator, TaskMergeAgentOutput, TaskMergeScope,
    TaskRunPhase, WorkCompletionRecord,
};

impl TaskCoordinator {
    pub(super) async fn load_merge_preflight_scope(
        &self,
        thread_id: &str,
        agent_id: &str,
    ) -> Result<TaskMergeScope> {
        let run = self
            .store
            .read_active_task_run_for_root_thread(thread_id)
            .await?;
        if !matches!(
            run.phase,
            TaskRunPhase::Implementing | TaskRunPhase::Reworking
        ) {
            bail!("task merge requires phase implementing or reworking");
        }
        let lease = self
            .store
            .read_branch_lease(&run.id)
            .await?
            .context("task branch lease not found")?;
        let work_units = self
            .store
            .list_work_units(&run.id)
            .await?
            .into_iter()
            .filter(|work_unit| work_unit.executor_thread_id.as_deref() == Some(agent_id))
            .collect::<Vec<_>>();
        let work_unit = match work_units.as_slice() {
            [work_unit] => work_unit.clone(),
            [] => bail!("approved executor work unit not found for Thread"),
            _ => bail!("ambiguous executor work unit for Thread"),
        };
        let completion = self
            .store
            .read_approved_work_completion(&work_unit.id)
            .await?;
        let delivery = delivery_from_completion(&completion)?;
        ensure_preflight_delivery_identity(&run.id, agent_id, &work_unit, &completion, &delivery)?;
        Ok(TaskMergeScope {
            run,
            lease,
            work_unit,
            completion,
            delivery,
            merge: empty_preflight_merge(),
        })
    }

    pub(super) async fn finish_accepted_delivery_cleanup(
        &self,
        scope: &TaskMergeScope,
        mut output: TaskMergeAgentOutput,
        runtime: Option<&AgentRuntimeHandle>,
    ) -> Result<TaskMergeAgentOutput> {
        if let Some(cleanup) = scope
            .merge
            .evidence
            .as_ref()
            .and_then(|evidence| evidence.cleanup.clone())
            && cleanup.status != "attempting"
        {
            output.cleanup = cleanup;
            return Ok(output);
        }
        self.store
            .record_merge_cleanup_attempting(&scope.merge.id)
            .await?;
        let cleanup = cleanup_accepted_delivery(scope, runtime).await;
        self.store
            .record_merge_cleanup(&scope.merge.id, cleanup.clone())
            .await?;
        output.cleanup = cleanup;
        Ok(output)
    }
}

pub(super) fn delivery_from_completion(completion: &WorkCompletionRecord) -> Result<AgentDelivery> {
    Ok(AgentDelivery {
        worktree: AgentWorktreeDelivery {
            path: completion.worktree_path.clone(),
            branch: completion.branch.clone(),
        },
        base_commit: completion.base_commit.clone(),
        head_commit: completion
            .head_commit
            .clone()
            .context("approved delivery completion has no head commit")?,
        changed_files: completion.changed_files.clone(),
        verification_summary: completion.verification_summary.clone(),
    })
}
