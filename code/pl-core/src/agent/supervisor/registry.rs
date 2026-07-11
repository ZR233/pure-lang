use pl_protocol::PureError;

use super::snapshot::unix_seconds;
use super::state::AgentEntry;
use crate::agent::worktree::{CloseDisposition, WorktreeRef};
use pl_protocol::SubAgentActivityKind;

use super::{
    AgentHandle, AgentPath, AgentRecord, AgentRunSpec, AgentSpawnInput, AgentSpawnLifecycleRequest,
    AgentStatus, AgentSupervisor,
};

impl AgentSupervisor {
    pub async fn spawn_agent(
        &self,
        input: AgentSpawnInput,
        mut run_spec: AgentRunSpec,
    ) -> Result<AgentHandle, PureError> {
        let parent_path = input
            .parent_path
            .clone()
            .unwrap_or_else(|| AgentPath::ROOT.to_string());
        let (mut handle, record) = {
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
            let id = super::identity::new_agent_id();
            let record = AgentRecord {
                id: id.clone(),
                path: path.to_string(),
                parent_path: Some(parent_path.clone()),
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
                    worktree: None,
                },
                record,
            )
        };
        let event_tx = run_spec.event_tx.clone();
        let call_id = run_spec.call_id.clone();
        self.notify_activity();

        let lifecycle_request = AgentSpawnLifecycleRequest {
            agent_id: handle.id.clone(),
            agent_path: handle.path.clone(),
            owner_path: parent_path,
            session_id: input.session_id,
            task_name: input.task_name,
            role: record.role.clone(),
            owned_paths: input.owned_paths,
            requested_by_call_id: call_id.clone(),
        };
        let lifecycle_hook = self.lifecycle_hook();
        let preparation = match &lifecycle_hook {
            Some(hook) => match hook.prepare_spawn(&lifecycle_request).await {
                Ok(preparation) => Some(preparation),
                Err(error) => {
                    let rollback_failures = self
                        .rollback_registered_spawn(&record.path, &handle.id)
                        .await
                        .err()
                        .into_iter()
                        .collect();
                    return Err(combine_spawn_and_rollback_errors(error, rollback_failures));
                }
            },
            None => None,
        };

        // 为 subagent 分配独立 worktree（启用 worktree 隔离时），并把 subagent 的
        // 工具世界钉到 worktree 路径，实现文件级物理隔离。
        let worktree_result = match &preparation {
            Some(preparation) => match preparation.worktree() {
                Some(spec) => Some(self.worktree.create_from_spec(spec.clone()).await),
                None => None,
            },
            None if self.worktree.is_enabled() => Some(self.worktree.create(&handle.id).await),
            None => None,
        };
        if let Some(result) = worktree_result {
            match result {
                Ok(worktree_handle) => {
                    let workspace_root = worktree_handle.path.clone();
                    let worktree_ref = WorktreeRef::from(&worktree_handle);
                    {
                        let mut state = self.state.lock().await;
                        if let Some(entry) = state.agents.get_mut(&handle.id) {
                            entry.worktree = Some(worktree_handle);
                        }
                    }
                    run_spec.workspace_root = workspace_root;
                    handle.worktree = Some(worktree_ref);
                }
                Err(error) => {
                    return Err(self
                        .rollback_spawn_lifecycle(
                            &record.path,
                            &handle.id,
                            lifecycle_hook.as_ref(),
                            &lifecycle_request,
                            preparation.as_ref(),
                            error.into(),
                        )
                        .await);
                }
            }
        }

        if let (Some(hook), Some(preparation)) = (&lifecycle_hook, &preparation)
            && let Err(error) = hook.activate_spawn(&lifecycle_request, preparation).await
        {
            return Err(self
                .rollback_spawn_lifecycle(
                    &record.path,
                    &handle.id,
                    lifecycle_hook.as_ref(),
                    &lifecycle_request,
                    Some(preparation),
                    error,
                )
                .await);
        }
        if let Some(lifecycle_token) = preparation
            .as_ref()
            .and_then(super::AgentSpawnPreparation::lifecycle_token)
        {
            let mut state = self.state.lock().await;
            if let Some(entry) = state.agents.get_mut(&handle.id) {
                entry.lifecycle_token = Some(lifecycle_token.to_string());
            }
        }

        if let Err(error) = self.start_agent_turn(handle.id.clone(), run_spec).await {
            return Err(self
                .rollback_spawn_lifecycle(
                    &record.path,
                    &handle.id,
                    lifecycle_hook.as_ref(),
                    &lifecycle_request,
                    preparation.as_ref(),
                    error,
                )
                .await);
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

    /// spawn 失败回滚：移除已注册 entry，并释放已分配的 worktree。
    async fn rollback_spawn_lifecycle(
        &self,
        path: &str,
        agent_id: &str,
        hook: Option<&std::sync::Arc<dyn super::AgentLifecycleHook>>,
        request: &AgentSpawnLifecycleRequest,
        preparation: Option<&super::AgentSpawnPreparation>,
        error: PureError,
    ) -> PureError {
        let lifecycle_error = error.to_string();
        let mut rollback_failures = Vec::new();
        if let Err(error) = self.rollback_registered_spawn(path, agent_id).await {
            rollback_failures.push(error);
        }
        if let (Some(hook), Some(preparation)) = (hook, preparation)
            && let Err(error) = hook
                .rollback_spawn(request, preparation, &lifecycle_error)
                .await
        {
            rollback_failures.push(error);
        }
        combine_spawn_and_rollback_errors(error, rollback_failures)
    }

    async fn rollback_registered_spawn(&self, path: &str, agent_id: &str) -> Result<(), PureError> {
        let worktree = {
            let mut state = self.state.lock().await;
            let worktree = state
                .agents
                .get(agent_id)
                .and_then(|entry| entry.worktree.clone());
            state.path_to_id.remove(path);
            state.agents.remove(agent_id);
            state.mark_activity();
            worktree
        };
        self.notify_activity();
        if let Some(handle) = worktree {
            self.worktree
                .close(&handle, CloseDisposition::Discard)
                .await
                .map_err(PureError::from)?;
        }
        Ok(())
    }

    pub async fn list_agents(
        &self,
        path_prefix: Option<&str>,
    ) -> Result<Vec<AgentRecord>, PureError> {
        let inputs = self.state.lock().await.agent_projection_inputs(path_prefix);
        let Some(hook) = self.lifecycle_hook() else {
            return Ok(inputs.into_iter().map(|(record, _)| record).collect());
        };
        let mut records = Vec::with_capacity(inputs.len());
        for (mut record, lifecycle_token) in inputs {
            let Some(lifecycle_token) = lifecycle_token else {
                records.push(record);
                continue;
            };
            let request = super::AgentLifecycleProjectionRequest {
                lifecycle_token,
                role: record.role.clone(),
                snapshot: super::AgentLifecycleProjection::new(
                    record.status,
                    record.summary.clone(),
                    record.error.clone(),
                ),
            };
            let projection = hook.project_snapshot(&request).await?;
            record.status = projection.status;
            record.summary = projection.summary;
            record.error = projection.error;
            records.push(record);
        }
        Ok(records)
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

fn combine_spawn_and_rollback_errors(
    error: PureError,
    rollback_failures: Vec<PureError>,
) -> PureError {
    if rollback_failures.is_empty() {
        return error;
    }
    let rollback_failures = rollback_failures
        .into_iter()
        .map(|failure| failure.to_string())
        .collect::<Vec<_>>()
        .join("; ");
    PureError::ToolExecutionFailed {
        tool: "spawn_agent".to_string(),
        error: format!("spawn failed: {error}; rollback failed: {rollback_failures}"),
    }
}
