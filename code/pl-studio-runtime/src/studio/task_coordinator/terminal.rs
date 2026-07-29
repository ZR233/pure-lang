use anyhow::Result;
use pl_core::TurnOutcomeKind;

use super::{AgentOutcomeStatus, DeliveryRecoveryNeed, TaskCoordinator};

/// Studio 任务层消费的 framework turn 终态事实。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StudioAgentTerminalChange {
    pub(crate) agent_id: String,
    pub(crate) role: String,
    pub(crate) outcome: TurnOutcomeKind,
    pub(crate) summary: Option<String>,
    pub(crate) error: Option<String>,
}

/// Studio durable task outcome 的产品投影。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StudioAgentOutcomeProjection {
    pub(crate) status: AgentOutcomeStatus,
    pub(crate) summary: Option<String>,
    pub(crate) error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TerminalAgentStateRecording {
    Unhandled,
    Changed {
        task_run_id: String,
        outcome_id: String,
        projection: StudioAgentOutcomeProjection,
    },
    Projected(StudioAgentOutcomeProjection),
    Suppressed,
}

impl TaskCoordinator {
    pub(crate) async fn inspect_delivery_recovery_need(
        &self,
        task_run_id: &str,
        agent_id: &str,
    ) -> Result<DeliveryRecoveryNeed> {
        let run = self
            .store
            .read_task_run(task_run_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("task run not found for delivery recovery"))?;
        let outcome = self
            .store
            .list_agent_outcomes(task_run_id)
            .await?
            .into_iter()
            .find(|outcome| outcome.agent_id == agent_id && outcome.role == "executor")
            .ok_or_else(|| anyhow::anyhow!("executor outcome not found for delivery recovery"))?;
        let work_unit_id = outcome
            .work_unit_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("executor delivery recovery work unit is missing"))?;
        let work_unit = self
            .store
            .read_work_unit(work_unit_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("executor delivery recovery work unit not found"))?;
        match super::git::inspect_executor_recovery(
            &work_unit.worktree_path,
            &work_unit.base_commit,
        )
        .await?
        {
            super::git::ExecutorRecoveryInspection::NoDelivery => {
                Ok(DeliveryRecoveryNeed::NoDelivery {
                    task_generation: run.task_generation,
                })
            }
            super::git::ExecutorRecoveryInspection::Recoverable => {
                Ok(DeliveryRecoveryNeed::Recoverable)
            }
        }
    }

    pub(crate) async fn record_terminal_agent_state(
        &self,
        session_id: &str,
        change: &StudioAgentTerminalChange,
    ) -> Result<TerminalAgentStateRecording> {
        let result = self
            .store
            .record_terminal_agent_state(session_id, change)
            .await?;
        if let TerminalAgentStateRecording::Changed { task_run_id, .. } = &result {
            self.publish_terminal_fact(task_run_id);
        }
        Ok(result)
    }

    pub(crate) async fn block_terminal_persistence_failure(
        &self,
        session_id: &str,
        error: &str,
    ) -> Result<()> {
        let Some(run) = self
            .store
            .list_active_task_runs()
            .await?
            .into_iter()
            .filter(|run| run.session_id == session_id)
            .max_by(|left, right| {
                left.updated_at
                    .cmp(&right.updated_at)
                    .then_with(|| left.id.cmp(&right.id))
            })
        else {
            return Ok(());
        };
        self.block_run(
            &run,
            format!("terminal agent state persistence failed: {error}"),
        )
        .await?;
        Ok(())
    }
}
