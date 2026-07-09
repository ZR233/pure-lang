use pl_protocol::{PureError, SubAgentActivityKind};
use tokio::task::JoinHandle;
use tokio::time::{Duration, timeout};

use super::snapshot::{apply_status_update, clear_for_reactivation};
use super::state::descendant_ids;
use super::{AgentRecord, AgentStatus, AgentStatusUpdate, AgentSupervisor};
use crate::agent::worktree::{CloseDisposition, WorktreeHandle};

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
        disposition: CloseDisposition,
    ) -> Result<AgentRecord, PureError> {
        let agent_id = self.resolve_agent(current_path, target).await?;
        let (record, mut shutdown) = {
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
            let mut shutdown = Vec::new();
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
                    let task = if entry.record.path != current_path {
                        entry.task.take()
                    } else {
                        None
                    };
                    let worktree = entry.worktree.clone();
                    let is_target = id == agent_id;
                    if is_target {
                        first = Some(entry.record.clone());
                    }
                    shutdown.push(AgentShutdownItem {
                        id,
                        worktree,
                        task,
                        is_target,
                    });
                }
            }
            state.mark_activity();
            (first.expect("target record exists"), shutdown)
        };
        self.notify_activity();

        let mut handles = Vec::new();
        for item in &mut shutdown {
            if let Some(handle) = item.task.take() {
                handles.push(handle);
            }
        }
        wait_for_agent_tasks(handles).await;

        // 释放 worktree：target 按 disposition（merge/discard），descendants 一律 discard。
        // target merge 冲突时不释放 worktree 并返回错误，调用方可调整后重试或改 discard。
        for item in &shutdown {
            let Some(worktree) = &item.worktree else {
                continue;
            };
            let item_disposition = if item.is_target {
                disposition.clone()
            } else {
                CloseDisposition::Discard
            };
            match self.worktree.close(worktree, item_disposition).await {
                Ok(_) => {
                    let mut state = self.state.lock().await;
                    if let Some(entry) = state.agents.get_mut(&item.id) {
                        entry.worktree = None;
                    }
                }
                Err(error) if item.is_target => {
                    return Err(error.into());
                }
                Err(_) => {
                    // descendant worktree 释放失败不阻断 close。
                }
            }
        }

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
        let (records, worktrees) = {
            let mut state = self.state.lock().await;
            let ids = descendant_ids(&state, agent_id);
            let mut records = Vec::new();
            let mut worktrees = Vec::new();
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
                    if let Some(worktree) = entry.worktree.clone() {
                        worktrees.push((id.clone(), worktree));
                    }
                    records.push(entry.record.clone());
                }
            }
            if !records.is_empty() {
                state.mark_activity();
            }
            (records, worktrees)
        };
        if !records.is_empty() {
            self.notify_activity();
        }
        wait_for_agent_tasks(handles).await;
        // 级联关闭的后代一律 discard 其 worktree（不自动 merge 未验收产物）。
        for (id, worktree) in worktrees {
            let _ = self
                .worktree
                .close(&worktree, CloseDisposition::Discard)
                .await;
            let mut state = self.state.lock().await;
            if let Some(entry) = state.agents.get_mut(&id) {
                entry.worktree = None;
            }
        }
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

/// close 时待释放的 agent 项：任务句柄、worktree 句柄与是否为目标 agent。
struct AgentShutdownItem {
    id: String,
    worktree: Option<WorktreeHandle>,
    task: Option<JoinHandle<()>>,
    is_target: bool,
}
