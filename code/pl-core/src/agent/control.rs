use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use pl_protocol::{BudgetLimitKind, BudgetUsage, PureError};
use tokio::sync::{Mutex, Notify};
use tokio_util::sync::CancellationToken;

use super::{AgentPath, AgentRecord, AgentStatus};
use crate::CoreSession;

/// Message queued for an existing agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentMailboxMessage {
    pub sender_path: String,
    pub message: String,
    pub trigger_turn: bool,
}

fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Delivery semantics for agent-to-agent messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageDeliveryMode {
    QueueOnly,
    TriggerTurn,
}

impl MessageDeliveryMode {
    pub fn trigger_turn(self) -> bool {
        matches!(self, Self::TriggerTurn)
    }
}

/// Input required to register a spawned agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSpawnInput {
    pub task_name: String,
    pub message: String,
    pub role: String,
    pub parent_path: Option<String>,
}

/// Handle returned by `AgentControl::spawn_agent`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentHandle {
    pub id: String,
    pub path: String,
    pub depth: u32,
}

/// Result of waiting for agent activity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentWaitOutcome {
    pub timed_out: bool,
    pub agents: Vec<AgentRecord>,
}

#[derive(Debug, Clone)]
pub struct AgentStatusUpdate {
    pub status: AgentStatus,
    pub summary: Option<String>,
    pub error: Option<String>,
    pub reason: Option<String>,
    pub budget_limit_kind: Option<BudgetLimitKind>,
    pub budget_usage: Option<BudgetUsage>,
}

impl AgentStatusUpdate {
    pub fn new(status: AgentStatus) -> Self {
        Self {
            status,
            summary: None,
            error: None,
            reason: None,
            budget_limit_kind: None,
            budget_usage: None,
        }
    }
}

#[derive(Debug, Clone)]
struct AgentEntry {
    record: AgentRecord,
    session: CoreSession,
    mailbox: VecDeque<AgentMailboxMessage>,
    cancellation_token: Option<CancellationToken>,
}

#[derive(Debug)]
struct AgentControlState {
    agents: HashMap<String, AgentEntry>,
    path_to_id: HashMap<String, String>,
    next_id: u64,
    max_agents: usize,
    max_depth: u32,
}

impl Default for AgentControlState {
    fn default() -> Self {
        let mut state = Self {
            agents: HashMap::new(),
            path_to_id: HashMap::new(),
            next_id: 0,
            max_agents: 16,
            max_depth: 3,
        };
        let root = AgentRecord {
            id: "agent-root".to_string(),
            path: AgentPath::ROOT.to_string(),
            parent_path: None,
            role: "root".to_string(),
            task: "root".to_string(),
            status: AgentStatus::Running,
            summary: None,
            error: None,
            reason: None,
            budget_limit_kind: None,
            budget_usage: None,
            depth: 0,
            updated_at: unix_seconds(),
        };
        state.path_to_id.insert(root.path.clone(), root.id.clone());
        state.agents.insert(
            root.id.clone(),
            AgentEntry {
                record: root,
                session: CoreSession::new(),
                mailbox: VecDeque::new(),
                cancellation_token: None,
            },
        );
        state
    }
}

/// In-memory coordinator for the current turn's agent tree.
///
/// The control object is shared by all collaboration tools registered on a
/// `PureCore` instance. It owns agent identity, status, mailbox and session
/// state; tools perform model execution but update this coordinator before
/// and after each asynchronous step.
#[derive(Debug, Clone, Default)]
pub struct AgentControl {
    state: Arc<Mutex<AgentControlState>>,
    notify: Arc<Notify>,
}

impl AgentControl {
    pub async fn configure_limits(&self, max_agents: usize, max_depth: u32) {
        let mut state = self.state.lock().await;
        state.max_agents = max_agents;
        state.max_depth = max_depth;
    }

    pub async fn spawn_agent(&self, input: AgentSpawnInput) -> Result<AgentHandle, PureError> {
        let parent_path = input
            .parent_path
            .unwrap_or_else(|| AgentPath::ROOT.to_string());
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
            return Err(PureError::ToolExecutionFailed {
                tool: "spawn_agent".to_string(),
                error: format!(
                    "budget limited by {} budget: agent nesting depth exceeds {}",
                    BudgetLimitKind::AgentDepth.as_str(),
                    state.max_depth
                ),
            });
        }
        if state.agents.len().saturating_sub(1) >= state.max_agents {
            return Err(PureError::ToolExecutionFailed {
                tool: "spawn_agent".to_string(),
                error: format!(
                    "budget limited by {} budget: agent limit reached {}",
                    BudgetLimitKind::AgentCount.as_str(),
                    state.max_agents
                ),
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
        let id = format!("agent-{}", state.next_id);
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
        state.agents.insert(
            id.clone(),
            AgentEntry {
                record,
                session: CoreSession::new(),
                mailbox: VecDeque::new(),
                cancellation_token: None,
            },
        );
        self.notify.notify_waiters();
        Ok(AgentHandle {
            id,
            path: path.to_string(),
            depth,
        })
    }

    pub async fn list_agents(&self, path_prefix: Option<&str>) -> Vec<AgentRecord> {
        let state = self.state.lock().await;
        let prefix = path_prefix
            .map(str::trim)
            .filter(|prefix| !prefix.is_empty());
        let mut agents: Vec<_> = state
            .agents
            .values()
            .map(|entry| entry.record.clone())
            .filter(|agent| {
                !agent.path.as_str().eq(AgentPath::ROOT)
                    && prefix.is_none_or(|prefix| agent.path.starts_with(prefix))
            })
            .collect();
        agents.sort_by(|left, right| left.path.cmp(&right.path));
        agents
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
        entry.record.status = update.status;
        if update.summary.is_some() {
            entry.record.summary = update.summary;
        }
        if update.error.is_some() {
            entry.record.error = update.error;
        }
        if update.reason.is_some() {
            entry.record.reason = update.reason;
        }
        if update.budget_limit_kind.is_some() {
            entry.record.budget_limit_kind = update.budget_limit_kind;
        }
        if update.budget_usage.is_some() {
            entry.record.budget_usage = update.budget_usage;
        }
        entry.record.updated_at = unix_seconds();
        let record = entry.record.clone();
        drop(state);
        self.notify.notify_waiters();
        Some(record)
    }

    pub async fn append_message(
        &self,
        current_path: &str,
        target: &str,
        message: String,
        mode: MessageDeliveryMode,
    ) -> Result<AgentRecord, PureError> {
        if message.trim().is_empty() {
            return Err(PureError::ToolExecutionFailed {
                tool: "send_message".to_string(),
                error: "message must not be empty".to_string(),
            });
        }
        let agent_id = self.resolve_agent(current_path, target).await?;
        let mut state = self.state.lock().await;
        let entry =
            state
                .agents
                .get_mut(&agent_id)
                .ok_or_else(|| PureError::ToolExecutionFailed {
                    tool: "send_message".to_string(),
                    error: format!("target agent not found: {target}"),
                })?;
        if entry.record.path == AgentPath::ROOT && mode == MessageDeliveryMode::TriggerTurn {
            return Err(PureError::ToolExecutionFailed {
                tool: "followup_task".to_string(),
                error: "tasks cannot be assigned to the root agent".to_string(),
            });
        }
        if entry.record.status.is_final() {
            let tool = if mode == MessageDeliveryMode::TriggerTurn {
                "followup_task"
            } else {
                "send_message"
            };
            return Err(PureError::ToolExecutionFailed {
                tool: tool.to_string(),
                error: format!(
                    "target agent {} is already {}",
                    entry.record.path,
                    entry.record.status.as_str()
                ),
            });
        }
        if mode == MessageDeliveryMode::TriggerTurn
            && matches!(
                entry.record.status,
                AgentStatus::Queued | AgentStatus::Running
            )
        {
            return Err(PureError::ToolExecutionFailed {
                tool: "followup_task".to_string(),
                error: format!(
                    "target agent {} is already {}",
                    entry.record.path,
                    entry.record.status.as_str()
                ),
            });
        }
        entry.mailbox.push_back(AgentMailboxMessage {
            sender_path: current_path.to_string(),
            message,
            trigger_turn: mode.trigger_turn(),
        });
        entry.record.status = if mode.trigger_turn() {
            AgentStatus::Queued
        } else {
            AgentStatus::Waiting
        };
        entry.record.updated_at = unix_seconds();
        let record = entry.record.clone();
        drop(state);
        self.notify.notify_waiters();
        Ok(record)
    }

    pub async fn take_trigger_message(&self, agent_id: &str) -> Option<String> {
        let mut state = self.state.lock().await;
        let entry = state.agents.get_mut(agent_id)?;
        let index = entry
            .mailbox
            .iter()
            .position(|message| message.trigger_turn)?;
        entry.mailbox.remove(index).map(|message| message.message)
    }

    pub async fn load_session(&self, agent_id: &str) -> Option<CoreSession> {
        self.state
            .lock()
            .await
            .agents
            .get(agent_id)
            .map(|entry| entry.session.clone())
    }

    pub async fn store_session(&self, agent_id: &str, session: CoreSession) {
        if let Some(entry) = self.state.lock().await.agents.get_mut(agent_id) {
            entry.session = session;
        }
    }

    pub async fn attach_cancellation_token(&self, agent_id: &str, token: CancellationToken) {
        if let Some(entry) = self.state.lock().await.agents.get_mut(agent_id) {
            entry.cancellation_token = Some(token);
        }
    }

    pub async fn close_agent(
        &self,
        current_path: &str,
        target: &str,
    ) -> Result<AgentRecord, PureError> {
        let agent_id = self.resolve_agent(current_path, target).await?;
        let mut state = self.state.lock().await;
        let entry =
            state
                .agents
                .get_mut(&agent_id)
                .ok_or_else(|| PureError::ToolExecutionFailed {
                    tool: "close_agent".to_string(),
                    error: format!("target agent not found: {target}"),
                })?;
        if entry.record.path == AgentPath::ROOT {
            return Err(PureError::ToolExecutionFailed {
                tool: "close_agent".to_string(),
                error: "root is not a spawned agent".to_string(),
            });
        }
        let record = entry.record.clone();
        let affected = descendant_ids(&state, &agent_id);
        for id in std::iter::once(agent_id.clone()).chain(affected) {
            if let Some(entry) = state.agents.get_mut(&id)
                && !entry.record.status.is_final()
            {
                entry.record.status = AgentStatus::Shutdown;
                entry.record.reason = Some("closed".to_string());
                entry.record.updated_at = unix_seconds();
                if let Some(token) = &entry.cancellation_token {
                    token.cancel();
                }
            }
        }
        drop(state);
        self.notify.notify_waiters();
        Ok(record)
    }

    pub async fn shutdown_descendants(&self, agent_id: &str, reason: &str) -> Vec<AgentRecord> {
        let mut state = self.state.lock().await;
        let ids = descendant_ids(&state, agent_id);
        let mut records = Vec::new();
        for id in ids {
            if let Some(entry) = state.agents.get_mut(&id)
                && !entry.record.status.is_final()
            {
                entry.record.status = AgentStatus::Shutdown;
                entry.record.reason = Some(reason.to_string());
                entry.record.updated_at = unix_seconds();
                if let Some(token) = &entry.cancellation_token {
                    token.cancel();
                }
                records.push(entry.record.clone());
            }
        }
        drop(state);
        if !records.is_empty() {
            self.notify.notify_waiters();
        }
        records
    }

    pub async fn wait_for_activity(&self, timeout_ms: i64) -> AgentWaitOutcome {
        let timeout_ms = timeout_ms.clamp(250, 120_000) as u64;
        let notified = tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            self.notify.notified(),
        )
        .await
        .is_ok();
        AgentWaitOutcome {
            timed_out: !notified,
            agents: self.list_agents(None).await,
        }
    }
}

fn descendant_ids(state: &AgentControlState, agent_id: &str) -> Vec<String> {
    let Some(root) = state.agents.get(agent_id) else {
        return Vec::new();
    };
    let prefix = format!("{}/", root.record.path);
    state
        .agents
        .iter()
        .filter_map(|(id, entry)| {
            (id != agent_id && entry.record.path.starts_with(&prefix)).then_some(id.clone())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[tokio::test]
    async fn spawns_lists_and_rejects_duplicate_paths() {
        let control = AgentControl::default();
        let input = AgentSpawnInput {
            task_name: "worker".to_string(),
            message: "inspect".to_string(),
            role: "explorer".to_string(),
            parent_path: None,
        };
        let handle = control.spawn_agent(input.clone()).await.unwrap();
        assert_eq!(handle.path, "/root/worker");
        assert!(control.spawn_agent(input).await.is_err());
        assert_eq!(control.list_agents(None).await.len(), 1);
    }

    #[tokio::test]
    async fn sends_and_closes_agent() {
        let control = AgentControl::default();
        control
            .spawn_agent(AgentSpawnInput {
                task_name: "worker".to_string(),
                message: "inspect".to_string(),
                role: "explorer".to_string(),
                parent_path: None,
            })
            .await
            .unwrap();
        let record = control
            .append_message(
                AgentPath::ROOT,
                "worker",
                "follow up".to_string(),
                MessageDeliveryMode::QueueOnly,
            )
            .await
            .unwrap();
        assert_eq!(record.status, AgentStatus::Waiting);
        let previous = control
            .close_agent(AgentPath::ROOT, "worker")
            .await
            .unwrap();
        assert_eq!(previous.status, AgentStatus::Waiting);
        assert_eq!(
            control.record(&previous.id).await.unwrap().status,
            AgentStatus::Shutdown
        );
        assert!(
            control
                .close_agent(AgentPath::ROOT, AgentPath::ROOT)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn append_message_rejects_final_agent_statuses() {
        let control = AgentControl::default();
        let handle = control
            .spawn_agent(AgentSpawnInput {
                task_name: "worker".to_string(),
                message: "inspect".to_string(),
                role: "explorer".to_string(),
                parent_path: None,
            })
            .await
            .unwrap();
        control
            .update_status(&handle.id, AgentStatus::Completed, None, None)
            .await
            .unwrap();

        let error = control
            .append_message(
                AgentPath::ROOT,
                "worker",
                "follow up".to_string(),
                MessageDeliveryMode::TriggerTurn,
            )
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("followup_task"));
        assert!(error.contains("already completed"));
        assert_eq!(
            control.record(&handle.id).await.unwrap().status,
            AgentStatus::Completed
        );
    }

    #[tokio::test]
    async fn followup_task_recovers_interrupted_agent() {
        let control = AgentControl::default();
        let handle = control
            .spawn_agent(AgentSpawnInput {
                task_name: "worker".to_string(),
                message: "inspect".to_string(),
                role: "explorer".to_string(),
                parent_path: None,
            })
            .await
            .unwrap();
        control
            .update_status(&handle.id, AgentStatus::Interrupted, None, None)
            .await
            .unwrap();

        let record = control
            .append_message(
                AgentPath::ROOT,
                "worker",
                "resume".to_string(),
                MessageDeliveryMode::TriggerTurn,
            )
            .await
            .unwrap();

        assert_eq!(record.status, AgentStatus::Queued);
    }

    #[tokio::test]
    async fn followup_task_rejects_running_or_queued_agent() {
        let control = AgentControl::default();
        let handle = control
            .spawn_agent(AgentSpawnInput {
                task_name: "worker".to_string(),
                message: "inspect".to_string(),
                role: "explorer".to_string(),
                parent_path: None,
            })
            .await
            .unwrap();
        let queued_error = control
            .append_message(
                AgentPath::ROOT,
                "worker",
                "queued follow up".to_string(),
                MessageDeliveryMode::TriggerTurn,
            )
            .await
            .unwrap_err()
            .to_string();
        control
            .update_status(&handle.id, AgentStatus::Running, None, None)
            .await
            .unwrap();
        let running_error = control
            .append_message(
                AgentPath::ROOT,
                "worker",
                "running follow up".to_string(),
                MessageDeliveryMode::TriggerTurn,
            )
            .await
            .unwrap_err()
            .to_string();

        assert!(queued_error.contains("already queued"));
        assert!(running_error.contains("already running"));
    }

    #[tokio::test]
    async fn spawn_reports_agent_budget_limits() {
        let control = AgentControl::default();
        control.configure_limits(0, 3).await;

        let error = control
            .spawn_agent(AgentSpawnInput {
                task_name: "worker".to_string(),
                message: "inspect".to_string(),
                role: "explorer".to_string(),
                parent_path: None,
            })
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("agentCount"));
    }

    #[tokio::test]
    async fn shutdown_descendants_marks_live_children() {
        let control = AgentControl::default();
        let parent = control
            .spawn_agent(AgentSpawnInput {
                task_name: "worker".to_string(),
                message: "inspect".to_string(),
                role: "explorer".to_string(),
                parent_path: None,
            })
            .await
            .unwrap();
        let child = control
            .spawn_agent(AgentSpawnInput {
                task_name: "child".to_string(),
                message: "inspect child".to_string(),
                role: "explorer".to_string(),
                parent_path: Some(parent.path),
            })
            .await
            .unwrap();

        let records = control
            .shutdown_descendants(&parent.id, "budgetLimited")
            .await;

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, child.id);
        assert_eq!(records[0].status, AgentStatus::Shutdown);
        assert_eq!(records[0].reason.as_deref(), Some("budgetLimited"));
    }
}
