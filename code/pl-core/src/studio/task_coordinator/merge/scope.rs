use anyhow::{Context, Result, bail};

use super::cleanup::cleanup_accepted_delivery;
use super::output::empty_preflight_merge;
use super::validation::ensure_preflight_delivery_identity;
use crate::AgentSupervisor;
use crate::studio::task_coordinator::{
    TaskCoordinator, TaskMergeAgentOutput, TaskMergeScope, TaskRunPhase,
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
            [] => bail!("delivered executor outcome not found for agent"),
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
        let delivery = outcome
            .delivery
            .clone()
            .context("completed executor outcome has no delivery")?;
        ensure_preflight_delivery_identity(&run.id, agent_id, &work_unit, &outcome, &delivery)?;
        Ok(TaskMergeScope {
            #[cfg(test)]
            origin_phase: run.phase,
            run,
            lease,
            work_unit,
            outcome,
            delivery,
            merge: empty_preflight_merge(),
        })
    }

    pub(super) async fn finish_accepted_delivery_cleanup(
        &self,
        scope: &TaskMergeScope,
        mut output: TaskMergeAgentOutput,
        supervisor: &AgentSupervisor,
        event_tx: &pl_trace::AgentEventSender,
        call_id: &str,
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
        self.pause_before_merge_cleanup().await;
        let cleanup = cleanup_accepted_delivery(scope, supervisor, event_tx, call_id).await;
        self.store
            .record_merge_cleanup(&scope.merge.id, cleanup.clone())
            .await?;
        output.cleanup = cleanup;
        Ok(output)
    }
}
