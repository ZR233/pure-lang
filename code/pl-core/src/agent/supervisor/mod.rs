use std::path::PathBuf;
use std::sync::Arc;

use pl_model::SharedModelProvider;
use pl_protocol::{BudgetLimitKind, BudgetUsage, PureError};
use tokio::sync::{Mutex, Notify};

use super::{AgentPath, AgentRecord, AgentStatus};
use crate::config::{PureConfig, ReasoningEffort};
use crate::turn::{CompileMode, TurnBudget, TurnOptions};

mod events;
mod execution;
mod lifecycle;
mod messaging;
mod registry;
mod runner;
mod snapshot;
mod state;
#[cfg(test)]
mod tests;
mod wait;

pub(crate) use events::emit_subagent_activity;

/// Message queued for an existing agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentMessage {
    pub sender_path: String,
    pub message: String,
    pub trigger_turn: bool,
}

/// Delivery semantics for agent-to-agent messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentMessageMode {
    QueueOnly,
    TriggerTurn,
}

impl AgentMessageMode {
    pub fn trigger_turn(self) -> bool {
        matches!(self, Self::TriggerTurn)
    }
}

/// Operation submitted by `send_message` or `followup_task`.
pub struct AgentMessageRequest<'a> {
    pub current_path: &'a str,
    pub target: &'a str,
    pub message: String,
    pub mode: AgentMessageMode,
    pub run_spec: Option<AgentRunSpec>,
    pub event_tx: &'a pl_trace::AgentEventSender,
    pub call_id: String,
}

/// Input required to register a spawned agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSpawnInput {
    pub task_name: String,
    pub message: String,
    pub role: String,
    pub parent_path: Option<String>,
}

/// Handle returned after an agent is registered.
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
pub struct AgentRunSpec {
    pub provider: SharedModelProvider,
    pub reasoning_effort: Option<ReasoningEffort>,
    pub config: Option<PureConfig>,
    pub mcp_runtime: Option<crate::mcp::McpRuntimeRegistry>,
    pub lsp_runtime: Option<pl_lsp::LspRuntimeRegistry>,
    pub workspace_instructions: Option<String>,
    pub instruction_snapshot: Option<crate::instruction::InstructionSnapshot>,
    pub workspace_root: PathBuf,
    pub options: TurnOptions,
    pub event_tx: pl_trace::AgentEventSender,
    pub call_id: String,
    pub message: String,
    pub mode: CompileMode,
    pub budget: TurnBudget,
    pub initial_session: crate::CoreSession,
}

/// Supervisor for the current root session's agent tree.
///
/// `AgentSupervisor` owns agent identity, latest snapshots, child sessions,
/// queued inter-agent messages, cancellation tokens and running task handles.
/// Collaboration tools submit operations to this type; they do not launch or
/// mutate child turns directly.
#[derive(Debug, Clone)]
pub struct AgentSupervisor {
    state: Arc<Mutex<state::AgentSupervisorState>>,
    notify: Arc<Notify>,
    execution: Arc<execution::AgentExecutionLimiter>,
}

impl Default for AgentSupervisor {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(state::AgentSupervisorState::default())),
            notify: Arc::new(Notify::new()),
            execution: Arc::new(execution::AgentExecutionLimiter::default()),
        }
    }
}

impl AgentSupervisor {
    pub async fn configure_limits(&self, max_agents: usize, max_depth: u32) {
        let mut state = self.state.lock().await;
        state.max_agents = max_agents;
        state.max_depth = max_depth;
        self.execution.configure(max_agents);
    }

    fn notify_activity(&self) {
        self.notify.notify_waiters();
    }

    async fn start_agent_turn(
        &self,
        agent_id: String,
        run_spec: AgentRunSpec,
    ) -> Result<(), PureError> {
        let guard = self.reserve_agent_execution()?;
        self.start_agent_turn_with_guard(agent_id, run_spec, guard)
            .await;
        Ok(())
    }

    fn reserve_agent_execution(&self) -> Result<execution::AgentExecutionGuard, PureError> {
        self.execution.guard()
    }

    async fn start_agent_turn_with_guard(
        &self,
        agent_id: String,
        run_spec: AgentRunSpec,
        guard: execution::AgentExecutionGuard,
    ) {
        let token = run_spec
            .options
            .cancellation_token
            .clone()
            .unwrap_or_default();
        let supervisor = self.clone();
        let task_agent_id = agent_id.clone();
        let handle = tokio::spawn(async move {
            let _guard = guard;
            runner::run_agent_turn(supervisor, task_agent_id, run_spec).await;
        });
        let mut state = self.state.lock().await;
        if let Some(entry) = state.agents.get_mut(&agent_id) {
            entry.cancellation_token = Some(token);
            entry.task = Some(handle);
        }
    }
}
