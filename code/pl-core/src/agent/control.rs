use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use pl_protocol::{BudgetLimitKind, PureError};
use tokio::sync::{Mutex, Notify};

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
struct AgentEntry {
    record: AgentRecord,
    session: CoreSession,
    mailbox: VecDeque<AgentMailboxMessage>,
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
        let mut state = self.state.lock().await;
        let entry = state.agents.get_mut(agent_id)?;
        entry.record.status = status;
        if summary.is_some() {
            entry.record.summary = summary;
        }
        if error.is_some() {
            entry.record.error = error;
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
        entry.record.status = AgentStatus::Closed;
        entry.record.updated_at = unix_seconds();
        let record = entry.record.clone();
        drop(state);
        self.notify.notify_waiters();
        Ok(record)
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
                MessageDeliveryMode::TriggerTurn,
            )
            .await
            .unwrap();
        assert_eq!(record.status, AgentStatus::Queued);
        let closed = control
            .close_agent(AgentPath::ROOT, "worker")
            .await
            .unwrap();
        assert_eq!(closed.status, AgentStatus::Closed);
        assert!(
            control
                .close_agent(AgentPath::ROOT, AgentPath::ROOT)
                .await
                .is_err()
        );
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
}
