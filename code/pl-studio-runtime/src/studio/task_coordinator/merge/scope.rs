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
        session_id: &str,
        agent_id: &str,
    ) -> Result<TaskMergeScope> {
        let run = self
            .store
            .read_active_task_run_for_session(session_id)
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
        let outcomes = self
            .store
            .list_agent_outcomes(&run.id)
            .await?
            .into_iter()
            .filter(|outcome| outcome.agent_id == agent_id)
            .collect::<Vec<_>>();
        let outcome = match outcomes.as_slice() {
            [outcome] => outcome.clone(),
            [] => bail!("approved executor outcome not found for agent"),
            _ => bail!("ambiguous executor outcome for agent"),
        };
        let work_unit_id = outcome
            .work_unit_id
            .as_deref()
            .context("executor outcome has no work unit")?;
        let work_unit = self
            .store
            .read_work_unit(work_unit_id)
            .await?
            .context("executor work unit not found")?;
        let completion = self
            .store
            .read_approved_work_completion(work_unit_id)
            .await?;
        let delivery = delivery_from_completion(&completion)?;
        ensure_preflight_delivery_identity(
            &run.id,
            agent_id,
            &work_unit,
            &outcome,
            &completion,
            &delivery,
        )?;
        Ok(TaskMergeScope {
            run,
            lease,
            work_unit,
            outcome,
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
