use std::time::Duration;

use anyhow::{Context, Result};

use super::TaskCoordinator;
use crate::{AgentRuntimeHandle, AgentState, ThreadId};

const CLOSED_PROJECTION_TIMEOUT: Duration = Duration::from_secs(5);
const CLOSED_PROJECTION_POLL_INTERVAL: Duration = Duration::from_millis(10);

impl TaskCoordinator {
    pub(super) async fn await_closed_agent_projection(
        &self,
        runtime: &AgentRuntimeHandle,
        agent_id: &str,
    ) -> Result<()> {
        let agent_id = ThreadId::new(agent_id.to_string())?;
        let snapshot = runtime
            .snapshot(agent_id.clone())
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        if !matches!(snapshot.state, AgentState::Closed(_)) {
            return Ok(());
        }
        self.await_closed_thread(agent_id.as_str()).await
    }

    pub(super) async fn await_closed_thread(&self, thread_id: &str) -> Result<()> {
        tokio::time::timeout(CLOSED_PROJECTION_TIMEOUT, async {
            loop {
                let thread =
                    self.store.read_thread(thread_id).await?.context(
                        "agent canonical Thread not found while awaiting close projection",
                    )?;
                if thread.status == pl_protocol::ThreadStatus::Closed {
                    return Ok::<(), anyhow::Error>(());
                }
                tokio::time::sleep(CLOSED_PROJECTION_POLL_INTERVAL).await;
            }
        })
        .await
        .with_context(|| format!("timed out waiting for agent `{thread_id}` close projection"))??;
        Ok(())
    }
}
