use anyhow::Result;

use crate::agent::{AgentLifecycleProjection, AgentTerminalStateChange};

use super::TaskCoordinator;

impl TaskCoordinator {
    pub(crate) async fn record_terminal_agent_state(
        &self,
        session_id: &str,
        change: &AgentTerminalStateChange,
    ) -> Result<Option<AgentLifecycleProjection>> {
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
