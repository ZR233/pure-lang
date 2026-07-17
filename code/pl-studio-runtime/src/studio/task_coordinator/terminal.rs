use anyhow::Result;
use pl_core::TurnOutcomeKind;

use super::{AgentOutcomeStatus, TaskCoordinator};

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
        projection: StudioAgentOutcomeProjection,
    },
    Projected(StudioAgentOutcomeProjection),
    Suppressed,
}

impl TaskCoordinator {
    pub(crate) async fn record_terminal_agent_state(
        &self,
        session_id: &str,
        change: &StudioAgentTerminalChange,
    ) -> Result<TerminalAgentStateRecording> {
        self.store
            .record_terminal_agent_state(session_id, change)
            .await
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
