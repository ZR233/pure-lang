use pl_protocol::{PureError, SubAgentActivityKind};
use tokio::task::JoinHandle;
use tokio::time::{Duration, timeout};

use super::snapshot::{apply_status_update, clear_for_reactivation};
use super::state::descendant_ids;
use super::{AgentRecord, AgentStatus, AgentStatusUpdate, AgentSupervisor};

const AGENT_SHUTDOWN_WAIT_TIMEOUT: Duration = Duration::from_secs(5);

impl AgentSupervisor {
    pub async fn update_status(
        &self,
        agent_id: &str,
        status: AgentStatus,
        summary: Option<String>,
        error: Option<String>,
    ) -> Option<AgentRecord> {
        self.update_status_with(
            agent_id,
            AgentStatusUpdate {
                status,
                summary,
                error,
                reason: None,
                budget_limit_kind: None,
                budget_usage: None,
            },
        )
        .await
    }

    pub async fn update_status_with(
        &self,
        agent_id: &str,
        update: AgentStatusUpdate,
    ) -> Option<AgentRecord> {
        let mut state = self.state.lock().await;
        let entry = state.agents.get_mut(agent_id)?;
        if entry.record.status.is_final() {
            return Some(entry.record.clone());
        }
        apply_status_update(&mut entry.record, update);
        let record = entry.record.clone();
        state.mark_activity();
        drop(state);
        self.notify_activity();
        Some(record)
    }

    pub async fn store_session(&self, agent_id: &str, session: crate::CoreSession) {
        if let Some(entry) = self.state.lock().await.agents.get_mut(agent_id) {
            entry.session = session;
        }
    }

    pub async fn load_session(&self, agent_id: &str) -> Option<crate::CoreSession> {
        self.state
            .lock()
            .await
            .agents
            .get(agent_id)
            .map(|entry| entry.session.clone())
    }

    pub async fn close_agent(
        &self,
        current_path: &str,
        target: &str,
        reason: &str,
        event_tx: &pl_trace::AgentEventSender,
        call_id: String,
    ) -> Result<AgentRecord, PureError> {
        let agent_id = self.resolve_agent(current_path, target).await?;
        let mut handles = Vec::new();
        let record = {
            let mut state = self.state.lock().await;
            let entry =
                state
                    .agents
                    .get(&agent_id)
                    .ok_or_else(|| PureError::ToolExecutionFailed {
                        tool: "close_agent".to_string(),
                        error: format!("target agent not found: {target}"),
                    })?;
            if entry.record.path == super::AgentPath::ROOT {
                return Err(PureError::ToolExecutionFailed {
                    tool: "close_agent".to_string(),
                    error: "root is not a spawned agent".to_string(),
                });
            }
            let ids = std::iter::once(agent_id.clone())
                .chain(descendant_ids(&state, &agent_id))
                .collect::<Vec<_>>();
            let mut first = None;
            for id in ids {
                if let Some(entry) = state.agents.get_mut(&id) {
                    if !entry.record.status.is_final() {
                        entry.record.status = AgentStatus::Shutdown;
                        entry.record.reason = Some(reason.to_string());
                        entry.record.updated_at = super::snapshot::unix_seconds();
                    }
                    if let Some(token) = &entry.cancellation_token {
                        token.cancel();
                    }
                    if entry.record.path != current_path
                        && let Some(handle) = entry.task.take()
                    {
                        handles.push(handle);
                    }
                    if id == agent_id {
                        first = Some(entry.record.clone());
                    }
                }
            }
            state.mark_activity();
            first.expect("target record exists")
        };
        self.notify_activity();
        wait_for_agent_tasks(handles).await;
        super::events::emit_agent_record(event_tx, &record);
        super::events::emit_subagent_activity(
            event_tx,
            call_id,
            Some(&record),
            SubAgentActivityKind::Closed,
            Some(reason.to_string()),
            None,
            None,
        );
        Ok(record)
    }

    pub async fn resume_agent(
        &self,
        current_path: &str,
        target: &str,
        event_tx: &pl_trace::AgentEventSender,
    ) -> Result<AgentRecord, PureError> {
        let agent_id = self.resolve_agent(current_path, target).await?;
        let record = {
            let mut state = self.state.lock().await;
            let Some(entry) = state.agents.get_mut(&agent_id) else {
                return Err(PureError::ToolExecutionFailed {
                    tool: "resume_agent".to_string(),
                    error: format!("target agent not found: {target}"),
                });
            };
            if entry.record.path == super::AgentPath::ROOT {
                return Err(PureError::ToolExecutionFailed {
                    tool: "resume_agent".to_string(),
                    error: "root is not a spawned agent".to_string(),
                });
            }
            if !entry.record.status.is_final() {
                return Err(PureError::ToolExecutionFailed {
                    tool: "resume_agent".to_string(),
                    error: format!(
                        "target agent {} is already {}",
                        entry.record.path,
                        entry.record.status.as_str()
                    ),
                });
            }
            entry.record.status = AgentStatus::Waiting;
            clear_for_reactivation(&mut entry.record);
            entry.cancellation_token = None;
            entry.task = None;
            let record = entry.record.clone();
            state.mark_activity();
            record
        };
        self.notify_activity();
        super::events::emit_agent_record(event_tx, &record);
        Ok(record)
    }

    pub async fn shutdown_descendants(&self, agent_id: &str, reason: &str) -> Vec<AgentRecord> {
        let mut handles = Vec::new();
        let records = {
            let mut state = self.state.lock().await;
            let ids = descendant_ids(&state, agent_id);
            let mut records = Vec::new();
            for id in ids {
                if let Some(entry) = state.agents.get_mut(&id)
                    && !entry.record.status.is_final()
                {
                    entry.record.status = AgentStatus::Shutdown;
                    entry.record.reason = Some(reason.to_string());
                    entry.record.updated_at = super::snapshot::unix_seconds();
                    if let Some(token) = &entry.cancellation_token {
                        token.cancel();
                    }
                    if let Some(handle) = entry.task.take() {
                        handles.push(handle);
                    }
                    records.push(entry.record.clone());
                }
            }
            if !records.is_empty() {
                state.mark_activity();
            }
            records
        };
        if !records.is_empty() {
            self.notify_activity();
        }
        wait_for_agent_tasks(handles).await;
        records
    }
}

async fn wait_for_agent_tasks(handles: Vec<JoinHandle<()>>) {
    for mut handle in handles {
        if timeout(AGENT_SHUTDOWN_WAIT_TIMEOUT, &mut handle)
            .await
            .is_err()
        {
            handle.abort();
            let _ = handle.await;
        }
    }
}
