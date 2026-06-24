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

#[derive(Debug, Clone)]
enum AgentStatusTransition {
    Update(AgentStatusUpdate),
    QueueFollowup,
    QueueMessage,
    Shutdown { reason: String },
}

impl AgentEntry {
    fn apply_status_transition(&mut self, transition: AgentStatusTransition) -> bool {
        if self.record.status.is_final() {
            return false;
        }
        match transition {
            AgentStatusTransition::Update(update) => self.apply_status_update(update),
            AgentStatusTransition::QueueFollowup => {
                self.record.status = AgentStatus::Queued;
                clear_status_details(&mut self.record);
            }
            AgentStatusTransition::QueueMessage => {
                if !matches!(
                    self.record.status,
                    AgentStatus::Queued | AgentStatus::Running
                ) {
                    self.record.status = AgentStatus::Waiting;
                }
            }
            AgentStatusTransition::Shutdown { reason } => {
                self.record.status = AgentStatus::Shutdown;
                self.record.reason = Some(reason);
            }
        }
        self.record.updated_at = unix_seconds();
        true
    }

    fn apply_status_update(&mut self, update: AgentStatusUpdate) {
        self.record.status = update.status;
        if clears_status_details(update.status) {
            clear_status_details(&mut self.record);
        }
        if update.summary.is_some() {
            self.record.summary = update.summary;
        }
        if update.error.is_some() {
            self.record.error = update.error;
        }
        if update.reason.is_some() {
            self.record.reason = update.reason;
        }
        if update.budget_limit_kind.is_some() {
            self.record.budget_limit_kind = update.budget_limit_kind;
        }
        if update.budget_usage.is_some() {
            self.record.budget_usage = update.budget_usage;
        }
    }
}

#[derive(Debug)]
struct AgentControlState {
    agents: HashMap<String, AgentEntry>,
    path_to_id: HashMap<String, String>,
    next_id: u64,
    max_agents: usize,
    max_depth: u32,
    activity_seq: u64,
    observed_activity_seq: u64,
}

impl Default for AgentControlState {
    fn default() -> Self {
        let mut state = Self {
            agents: HashMap::new(),
            path_to_id: HashMap::new(),
            next_id: 0,
            max_agents: 16,
            max_depth: 3,
            activity_seq: 0,
            observed_activity_seq: 0,
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

impl AgentControlState {
    fn mark_activity(&mut self) {
        self.activity_seq = self.activity_seq.saturating_add(1);
    }

    fn has_final_agent(&self) -> bool {
        self.agents.values().any(|entry| {
            !entry.record.path.as_str().eq(AgentPath::ROOT) && entry.record.status.is_final()
        })
    }

    fn agent_records(&self, path_prefix: Option<&str>) -> Vec<AgentRecord> {
        let prefix = path_prefix
            .map(str::trim)
            .filter(|prefix| !prefix.is_empty());
        let mut agents: Vec<_> = self
            .agents
            .values()
            .map(|entry| entry.record.clone())
            .filter(|agent| {
                !agent.path.as_str().eq(AgentPath::ROOT)
                    && prefix.is_none_or(|prefix| path_matches_prefix(&agent.path, prefix))
            })
            .collect();
        agents.sort_by(|left, right| left.path.cmp(&right.path));
        agents
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
        state.mark_activity();
        self.notify.notify_waiters();
        Ok(AgentHandle {
            id,
            path: path.to_string(),
            depth,
        })
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
        if !entry.apply_status_transition(AgentStatusTransition::Update(update)) {
            return Some(entry.record.clone());
        }
        let record = entry.record.clone();
        state.mark_activity();
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
        if mode.trigger_turn() {
            entry.apply_status_transition(AgentStatusTransition::QueueFollowup);
        } else {
            entry.apply_status_transition(AgentStatusTransition::QueueMessage);
        }
        let record = entry.record.clone();
        state.mark_activity();
        drop(state);
        self.notify.notify_waiters();
        Ok(record)
    }

    pub async fn take_turn_messages(&self, agent_id: &str) -> Option<Vec<AgentMailboxMessage>> {
        let mut state = self.state.lock().await;
        let entry = state.agents.get_mut(agent_id)?;
        entry
            .mailbox
            .iter()
            .any(|message| message.trigger_turn)
            .then(|| entry.mailbox.drain(..).collect())
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
                && entry.apply_status_transition(AgentStatusTransition::Shutdown {
                    reason: "closed".to_string(),
                })
                && let Some(token) = &entry.cancellation_token
            {
                token.cancel();
            }
        }
        state.mark_activity();
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
                && entry.apply_status_transition(AgentStatusTransition::Shutdown {
                    reason: reason.to_string(),
                })
            {
                if let Some(token) = &entry.cancellation_token {
                    token.cancel();
                }
                records.push(entry.record.clone());
            }
        }
        if !records.is_empty() {
            state.mark_activity();
            drop(state);
            self.notify.notify_waiters();
        } else {
            drop(state);
        }
        records
    }

    pub async fn wait_for_activity(&self, timeout_ms: i64) -> AgentWaitOutcome {
        use tokio::time::{Duration, Instant};

        let timeout_ms = timeout_ms.clamp(250, 120_000) as u64;
        let start_seq = {
            let mut state = self.state.lock().await;
            if state.observed_activity_seq == 0 {
                if state.has_final_agent() {
                    state.observed_activity_seq = state.activity_seq;
                    return AgentWaitOutcome {
                        timed_out: false,
                        agents: state.agent_records(None),
                    };
                }
                state.activity_seq
            } else if state.activity_seq > state.observed_activity_seq {
                state.observed_activity_seq = state.activity_seq;
                return AgentWaitOutcome {
                    timed_out: false,
                    agents: state.agent_records(None),
                };
            } else {
                state.observed_activity_seq
            }
        };
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        loop {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let mut state = self.state.lock().await;
                if state.activity_seq > start_seq {
                    state.observed_activity_seq = state.activity_seq;
                    return AgentWaitOutcome {
                        timed_out: false,
                        agents: state.agent_records(None),
                    };
                }
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                return AgentWaitOutcome {
                    timed_out: true,
                    agents: self.list_agents(None).await,
                };
            }
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

fn path_matches_prefix(path: &str, prefix: &str) -> bool {
    path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn clears_status_details(status: AgentStatus) -> bool {
    matches!(
        status,
        AgentStatus::Queued | AgentStatus::Running | AgentStatus::Completed
    )
}

fn clear_status_details(record: &mut AgentRecord) {
    record.error = None;
    record.reason = None;
    record.budget_limit_kind = None;
    record.budget_usage = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    async fn spawn(control: &AgentControl, task_name: &str) -> AgentHandle {
        control
            .spawn_agent(AgentSpawnInput {
                task_name: task_name.to_string(),
                message: format!("inspect {task_name}"),
                role: "explorer".to_string(),
                parent_path: None,
            })
            .await
            .unwrap()
    }

    async fn spawn_child(
        control: &AgentControl,
        parent_path: &str,
        task_name: &str,
    ) -> AgentHandle {
        control
            .spawn_agent(AgentSpawnInput {
                task_name: task_name.to_string(),
                message: format!("inspect {task_name}"),
                role: "explorer".to_string(),
                parent_path: Some(parent_path.to_string()),
            })
            .await
            .unwrap()
    }

    async fn update_status(
        control: &AgentControl,
        agent_id: &str,
        status: AgentStatus,
    ) -> AgentRecord {
        control
            .update_status(agent_id, status, None, None)
            .await
            .unwrap()
    }

    async fn append_message(
        control: &AgentControl,
        target: &str,
        message: &str,
        mode: MessageDeliveryMode,
    ) -> Result<AgentRecord, PureError> {
        control
            .append_message(AgentPath::ROOT, target, message.to_string(), mode)
            .await
    }

    fn root_message(message: &str, trigger_turn: bool) -> AgentMailboxMessage {
        AgentMailboxMessage {
            sender_path: AgentPath::ROOT.to_string(),
            message: message.to_string(),
            trigger_turn,
        }
    }

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
    async fn list_agents_path_prefix_matches_subtree_boundaries() {
        let control = AgentControl::default();
        let a = spawn(&control, "a").await;
        spawn_child(&control, &a.path, "child").await;
        spawn(&control, "ab").await;

        let agents = control.list_agents(Some("/root/a")).await;

        assert_eq!(
            agents
                .into_iter()
                .map(|agent| agent.path)
                .collect::<Vec<_>>(),
            vec!["/root/a".to_string(), "/root/a/child".to_string()]
        );
    }

    #[tokio::test]
    async fn sends_and_closes_agent() {
        let control = AgentControl::default();
        let handle = spawn(&control, "worker").await;
        update_status(&control, &handle.id, AgentStatus::Interrupted).await;
        let record = append_message(
            &control,
            "worker",
            "follow up",
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
    async fn send_message_preserves_queued_or_running_status() {
        let control = AgentControl::default();
        let handle = spawn(&control, "worker").await;

        let queued = append_message(
            &control,
            "worker",
            "queued context",
            MessageDeliveryMode::QueueOnly,
        )
        .await
        .unwrap();
        update_status(&control, &handle.id, AgentStatus::Running).await;
        let running = append_message(
            &control,
            "worker",
            "running context",
            MessageDeliveryMode::QueueOnly,
        )
        .await
        .unwrap();

        assert_eq!(queued.status, AgentStatus::Queued);
        assert_eq!(running.status, AgentStatus::Running);
    }

    #[tokio::test]
    async fn send_message_preserves_interruption_details_without_triggering_turn() {
        let control = AgentControl::default();
        let handle = spawn(&control, "worker").await;
        control
            .update_status_with(
                &handle.id,
                AgentStatusUpdate {
                    status: AgentStatus::Interrupted,
                    summary: None,
                    error: Some("budget used".to_string()),
                    reason: Some("budgetLimited".to_string()),
                    budget_limit_kind: Some(BudgetLimitKind::ModelStep),
                    budget_usage: Some(BudgetUsage {
                        model_steps: 1,
                        tool_calls: 0,
                        wait_calls: 0,
                        elapsed_ms: 10,
                    }),
                },
            )
            .await
            .unwrap();

        let record = append_message(
            &control,
            "worker",
            "context",
            MessageDeliveryMode::QueueOnly,
        )
        .await
        .unwrap();

        assert_eq!(record.status, AgentStatus::Waiting);
        assert_eq!(record.error.as_deref(), Some("budget used"));
        assert_eq!(record.reason.as_deref(), Some("budgetLimited"));
        assert_eq!(record.budget_limit_kind, Some(BudgetLimitKind::ModelStep));
        assert!(record.budget_usage.is_some());
    }

    #[tokio::test]
    async fn append_message_rejects_final_agent_statuses() {
        let control = AgentControl::default();
        let handle = spawn(&control, "worker").await;
        update_status(&control, &handle.id, AgentStatus::Completed).await;

        let error = append_message(
            &control,
            "worker",
            "follow up",
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
    async fn followup_task_clears_stale_interruption_details() {
        let control = AgentControl::default();
        let handle = spawn(&control, "worker").await;
        control
            .update_status_with(
                &handle.id,
                AgentStatusUpdate {
                    status: AgentStatus::Interrupted,
                    summary: None,
                    error: Some("budget used".to_string()),
                    reason: Some("budgetLimited".to_string()),
                    budget_limit_kind: Some(BudgetLimitKind::ModelStep),
                    budget_usage: Some(BudgetUsage {
                        model_steps: 1,
                        tool_calls: 0,
                        wait_calls: 0,
                        elapsed_ms: 10,
                    }),
                },
            )
            .await
            .unwrap();

        let queued = append_message(
            &control,
            "worker",
            "resume",
            MessageDeliveryMode::TriggerTurn,
        )
        .await
        .unwrap();
        assert_eq!(queued.status, AgentStatus::Queued);
        let running = update_status(&control, &handle.id, AgentStatus::Running).await;
        let completed = control
            .update_status(
                &handle.id,
                AgentStatus::Completed,
                Some("done".to_string()),
                None,
            )
            .await
            .unwrap();

        for record in [queued, running, completed] {
            assert_eq!(record.error, None);
            assert_eq!(record.reason, None);
            assert_eq!(record.budget_limit_kind, None);
            assert_eq!(record.budget_usage, None);
        }
    }

    #[tokio::test]
    async fn take_turn_messages_drains_queued_messages_with_trigger() {
        let control = AgentControl::default();
        let handle = spawn(&control, "worker").await;
        update_status(&control, &handle.id, AgentStatus::Interrupted).await;
        append_message(
            &control,
            "worker",
            "context",
            MessageDeliveryMode::QueueOnly,
        )
        .await
        .unwrap();
        assert!(control.take_turn_messages(&handle.id).await.is_none());
        append_message(
            &control,
            "worker",
            "resume",
            MessageDeliveryMode::TriggerTurn,
        )
        .await
        .unwrap();
        append_message(
            &control,
            "worker",
            "late context",
            MessageDeliveryMode::QueueOnly,
        )
        .await
        .unwrap();

        let messages = control.take_turn_messages(&handle.id).await.unwrap();

        assert_eq!(
            messages,
            vec![
                root_message("context", false),
                root_message("resume", true),
                root_message("late context", false),
            ]
        );
        assert!(control.take_turn_messages(&handle.id).await.is_none());
    }

    #[tokio::test]
    async fn followup_task_rejects_running_or_queued_agent() {
        let control = AgentControl::default();
        let handle = spawn(&control, "worker").await;
        let queued_error = append_message(
            &control,
            "worker",
            "queued follow up",
            MessageDeliveryMode::TriggerTurn,
        )
        .await
        .unwrap_err()
        .to_string();
        update_status(&control, &handle.id, AgentStatus::Running).await;
        let running_error = append_message(
            &control,
            "worker",
            "running follow up",
            MessageDeliveryMode::TriggerTurn,
        )
        .await
        .unwrap_err()
        .to_string();

        assert!(queued_error.contains("already queued"));
        assert!(running_error.contains("already running"));
    }

    #[tokio::test]
    async fn wait_for_activity_returns_completed_snapshot_without_new_notify() {
        let control = AgentControl::default();
        let handle = spawn(&control, "worker").await;
        control
            .update_status(
                &handle.id,
                AgentStatus::Completed,
                Some("done".to_string()),
                None,
            )
            .await
            .unwrap();

        let outcome = control.wait_for_activity(250).await;

        assert!(!outcome.timed_out);
        assert_eq!(outcome.agents.len(), 1);
        assert_eq!(outcome.agents[0].status, AgentStatus::Completed);
        assert_eq!(outcome.agents[0].summary.as_deref(), Some("done"));
    }

    #[tokio::test]
    async fn wait_for_activity_does_not_replay_old_final_agents() {
        let control = AgentControl::default();
        let first = spawn(&control, "first").await;
        let second = spawn(&control, "second").await;
        update_status(&control, &first.id, AgentStatus::Completed).await;
        update_status(&control, &second.id, AgentStatus::Running).await;
        assert!(!control.wait_for_activity(250).await.timed_out);

        let before = tokio::time::Instant::now();
        let outcome = control.wait_for_activity(250).await;

        assert!(outcome.timed_out);
        assert!(before.elapsed() >= std::time::Duration::from_millis(200));
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
        let parent = spawn(&control, "worker").await;
        let child = spawn_child(&control, &parent.path, "child").await;
        control
            .update_status_with(
                &child.id,
                AgentStatusUpdate {
                    status: AgentStatus::Interrupted,
                    summary: None,
                    error: Some("needs cleanup".to_string()),
                    reason: Some("budgetLimited".to_string()),
                    budget_limit_kind: Some(BudgetLimitKind::WallClock),
                    budget_usage: Some(BudgetUsage {
                        model_steps: 1,
                        tool_calls: 2,
                        wait_calls: 3,
                        elapsed_ms: 4,
                    }),
                },
            )
            .await
            .unwrap();

        let records = control
            .shutdown_descendants(&parent.id, "budgetLimited")
            .await;

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, child.id);
        assert_eq!(records[0].status, AgentStatus::Shutdown);
        assert_eq!(records[0].error.as_deref(), Some("needs cleanup"));
        assert_eq!(records[0].reason.as_deref(), Some("budgetLimited"));
        assert_eq!(
            records[0].budget_limit_kind,
            Some(BudgetLimitKind::WallClock)
        );
        assert!(records[0].budget_usage.is_some());
    }
}
