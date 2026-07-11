use anyhow::Result;

use crate::agent::{AgentLifecycleProjection, AgentTerminalStateChange};

use super::TaskCoordinator;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TerminalAgentStateRecording {
    Unhandled,
    Changed {
        task_run_id: String,
        projection: AgentLifecycleProjection,
    },
    Projected(AgentLifecycleProjection),
    Suppressed,
}

impl TerminalAgentStateRecording {
    #[cfg(test)]
    pub(crate) fn into_projection(self) -> Option<AgentLifecycleProjection> {
        match self {
            Self::Changed { projection, .. } | Self::Projected(projection) => Some(projection),
            Self::Unhandled | Self::Suppressed => None,
        }
    }
}

impl TaskCoordinator {
    pub(crate) async fn project_agent_activity(
        &self,
        session_id: &str,
        agent_id: &str,
    ) -> Result<Option<AgentLifecycleProjection>> {
        self.store
            .project_agent_activity(session_id, agent_id)
            .await
    }

    pub(crate) async fn record_terminal_agent_state(
        &self,
        session_id: &str,
        change: &AgentTerminalStateChange,
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
