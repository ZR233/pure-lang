use pl_protocol::PureError;

use super::snapshot::unix_seconds;
use super::state::AgentEntry;
use pl_protocol::SubAgentActivityKind;

use super::{
    AgentHandle, AgentPath, AgentRecord, AgentRunSpec, AgentSpawnInput, AgentStatus,
    AgentSupervisor,
};

impl AgentSupervisor {
    pub async fn spawn_agent(
        &self,
        input: AgentSpawnInput,
        run_spec: AgentRunSpec,
    ) -> Result<AgentHandle, PureError> {
        let parent_path = input
            .parent_path
            .unwrap_or_else(|| AgentPath::ROOT.to_string());
        let (handle, record) = {
            let mut state = self.state.lock().await;
            let parent = state
                .path_to_id
                .get(&parent_path)
                .and_then(|id| state.agents.get(id))
                .ok_or_else(|| PureError::ToolExecutionFailed {
                    tool: "spawn_agent".to_string(),
                    error: format!("parent agent not found: {parent_path}"),
                })?;
            let depth = parent.record.depth + 1;
            if depth > state.max_depth {
                return Err(PureError::AgentDepthLimitReached {
                    max_depth: state.max_depth,
                });
            }
            if state.agents.len().saturating_sub(1) >= state.max_agents {
                return Err(PureError::AgentLimitReached {
                    max_agents: state.max_agents,
                });
            }
            let path = AgentPath::try_from(parent_path.as_str())
                .and_then(|parent| parent.join(&input.task_name))
                .map_err(|error| PureError::ToolExecutionFailed {
                    tool: "spawn_agent".to_string(),
                    error,
                })?;
            if state.path_to_id.contains_key(path.as_str()) {
                return Err(PureError::ToolExecutionFailed {
                    tool: "spawn_agent".to_string(),
                    error: format!("agent path already exists: {path}"),
                });
            }
            state.next_id += 1;
            let next_id = state.next_id;
            let id = format!("agent-{next_id}");
            let record = AgentRecord {
                id: id.clone(),
                path: path.to_string(),
                parent_path: Some(parent_path),
                role: input.role,
                task: input.message,
                status: AgentStatus::Queued,
                summary: None,
                error: None,
                reason: None,
                budget_limit_kind: None,
                budget_usage: None,
                depth,
                updated_at: unix_seconds(),
            };
            state.path_to_id.insert(record.path.clone(), id.clone());
            let mut entry = AgentEntry::new(record.clone());
            entry.session = run_spec.initial_session.clone();
            state.agents.insert(id.clone(), entry);
            state.mark_activity();
            (
                AgentHandle {
                    id,
                    path: path.to_string(),
                    depth,
                },
                record,
            )
        };
        let event_tx = run_spec.event_tx.clone();
        let call_id = run_spec.call_id.clone();
        self.notify_activity();
        if let Err(error) = self.start_agent_turn(handle.id.clone(), run_spec).await {
            let mut state = self.state.lock().await;
            state.path_to_id.remove(&record.path);
            state.agents.remove(&handle.id);
            state.mark_activity();
            drop(state);
            self.notify_activity();
            return Err(error);
        }
        super::events::emit_agent_record(&event_tx, &record);
        super::events::emit_subagent_activity(
            &event_tx,
            call_id,
            Some(&record),
            SubAgentActivityKind::Spawned,
            Some(record.task.clone()),
            None,
            None,
        );
        Ok(handle)
    }

    pub async fn list_agents(&self, path_prefix: Option<&str>) -> Vec<AgentRecord> {
        let state = self.state.lock().await;
        state.agent_records(path_prefix)
    }

    pub async fn resolve_agent(
        &self,
        current_path: &str,
        target: &str,
    ) -> Result<String, PureError> {
        let state = self.state.lock().await;
        if let Some(id) = state.path_to_id.get(target) {
            return Ok(id.clone());
        }
        if let Some(entry) = state.agents.get(target) {
            return Ok(entry.record.id.clone());
        }
        let current = AgentPath::try_from(current_path).unwrap_or_else(|_| AgentPath::root());
        let path = current
            .resolve(target)
            .map_err(|error| PureError::ToolExecutionFailed {
                tool: "agent".to_string(),
                error,
            })?;
        state
            .path_to_id
            .get(path.as_str())
            .cloned()
            .ok_or_else(|| PureError::ToolExecutionFailed {
                tool: "agent".to_string(),
                error: format!("target agent not found: {target}"),
            })
    }

    pub async fn record(&self, agent_id: &str) -> Option<AgentRecord> {
        self.state
            .lock()
            .await
            .agents
            .get(agent_id)
            .map(|entry| entry.record.clone())
    }
}
