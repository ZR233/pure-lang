use std::collections::VecDeque;
use std::path::PathBuf;
use std::pin::Pin;
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
pub use wait::{
    AgentWaitLoopError, AgentWaitLoopOptions, AgentWaitLoopResult, wait_for_agent_completion,
};

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

/// `send_input` 的通用 turn 策略。
///
/// 该类型把模型可见的 `triggerTurn` / `interrupt` 标志收敛成一个明确模式，
/// 宿主复用它决定是否只排队、启动新 turn，或先中断目标 agent 再启动。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentInputTurnMode {
    QueueOnly,
    TriggerTurn,
    Interrupt,
}

impl AgentInputTurnMode {
    pub fn from_codex_flags(trigger_turn: bool, interrupt: bool) -> Self {
        if interrupt {
            Self::Interrupt
        } else if trigger_turn {
            Self::TriggerTurn
        } else {
            Self::QueueOnly
        }
    }

    /// 计算收到输入后的第一步动作。
    pub fn initial_action(self) -> AgentInputInitialAction {
        match self {
            Self::QueueOnly => AgentInputInitialAction::Queue,
            Self::TriggerTurn => AgentInputInitialAction::StartTurn,
            Self::Interrupt => AgentInputInitialAction::InterruptThenStart,
        }
    }

    /// 计算启动 turn 时遇到 busy 的处理方式。
    pub fn busy_action(self) -> AgentInputBusyAction {
        match self {
            Self::QueueOnly | Self::Interrupt => AgentInputBusyAction::ReturnBusy,
            Self::TriggerTurn => AgentInputBusyAction::Queue,
        }
    }

    pub fn queues_without_start(self) -> bool {
        matches!(self.initial_action(), AgentInputInitialAction::Queue)
    }

    pub fn interrupts(self) -> bool {
        matches!(
            self.initial_action(),
            AgentInputInitialAction::InterruptThenStart
        )
    }

    pub fn queues_when_busy(self) -> bool {
        matches!(self.busy_action(), AgentInputBusyAction::Queue)
    }
}

/// `send_input` 收到输入后的共享第一步动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentInputInitialAction {
    Queue,
    StartTurn,
    InterruptThenStart,
}

/// `send_input` 启动 turn 时遇到 busy 的共享处理方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentInputBusyAction {
    Queue,
    ReturnBusy,
}

/// `send_input` 提交输入后的共享结果。
///
/// 宿主可以用该类型在自身队列/turn 启动逻辑之间传递结果，避免用裸 JSON 表达
/// `queued` / `turnId` 这类模型可见工具输出字段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentInputSubmission {
    queued: bool,
    turn_id: Option<String>,
}

impl AgentInputSubmission {
    pub fn queued() -> Self {
        Self {
            queued: true,
            turn_id: None,
        }
    }

    pub fn started(turn_id: impl Into<String>) -> Self {
        Self {
            queued: false,
            turn_id: Some(turn_id.into()),
        }
    }

    pub fn queued_flag(&self) -> bool {
        self.queued
    }

    pub fn turn_id(&self) -> Option<&str> {
        self.turn_id.as_deref()
    }

    pub fn into_send_input_output(
        self,
        target: String,
        status: AgentStatus,
        interrupt: bool,
    ) -> crate::tool::AgentControlSendInputOutput {
        crate::tool::AgentControlSendInputOutput {
            target,
            status,
            interrupt,
            queued: self.queued,
            turn_id: self.turn_id,
        }
    }
}

/// agent 输入队列，负责保持 pending input 的 FIFO 与重试顺序。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentInputQueue<Input> {
    pending: VecDeque<Input>,
}

/// 从 pending input 队列取出的启动尝试。
///
/// 宿主可以在不持有队列锁的情况下解析 session、准备 turn 或执行产品检查；
/// 如果启动发现目标仍忙，应把该 attempt 交回 `restore_start_attempt`，
/// 由 pl-core 统一恢复到队首，保持重试顺序。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentInputStartAttempt<Input> {
    input: Input,
}

impl<Input> AgentInputStartAttempt<Input> {
    pub fn input(&self) -> &Input {
        &self.input
    }

    pub fn into_input(self) -> Input {
        self.input
    }
}

impl<Input> AgentInputQueue<Input> {
    pub fn new() -> Self {
        Self {
            pending: VecDeque::new(),
        }
    }

    pub fn push(&mut self, input: Input) {
        self.pending.push_back(input);
    }

    pub fn pop(&mut self) -> Option<Input> {
        self.pending.pop_front()
    }

    pub fn restore_front(&mut self, input: Input) {
        self.pending.push_front(input);
    }

    pub fn take_start_attempt(&mut self) -> Option<AgentInputStartAttempt<Input>> {
        self.pending
            .pop_front()
            .map(|input| AgentInputStartAttempt { input })
    }

    pub fn restore_start_attempt(&mut self, attempt: AgentInputStartAttempt<Input>) {
        self.pending.push_front(attempt.into_input());
    }

    pub fn len(&self) -> usize {
        self.pending.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

impl<Input> Default for AgentInputQueue<Input> {
    fn default() -> Self {
        Self::new()
    }
}

/// Operation submitted by `send_input`.
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

impl AgentWaitOutcome {
    /// 将共享 wait 结果投影为模型可见的 `wait_agent` 输出。
    ///
    /// 宿主仍可决定 message 中包含哪些产品诊断信息；`timedOut` 字段和输出形状
    /// 由 pl-core 统一维护，避免宿主 adapter 解析或重建共享工具字段。
    pub fn into_wait_agent_output(
        self,
        message: impl Into<String>,
    ) -> crate::tool::AgentControlWaitOutput {
        crate::tool::AgentControlWaitOutput {
            message: message.into(),
            timed_out: self.timed_out,
        }
    }
}

/// 宿主 agent 当前是否仍持有 active turn。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentTurnPresence {
    ActiveTurn,
    NoActiveTurn,
}

/// agent 生命周期判断所需的宿主无关状态分类。
///
/// 不同产品可拥有自己的状态 enum；接入层只需要把产品状态映射到该分类，
/// wait 完成和 turn 启动等通用生命周期规则则由 pl-core 统一维护。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentLifecycleStatusKind {
    Active,
    Idle,
    Completed,
    Failed,
    Cancelled,
    Deleted,
}

impl AgentLifecycleStatusKind {
    fn is_wait_completion_status(self) -> bool {
        matches!(
            self,
            Self::Idle | Self::Completed | Self::Failed | Self::Cancelled | Self::Deleted
        )
    }

    fn is_turn_start_ready(self) -> bool {
        matches!(
            self,
            Self::Idle | Self::Completed | Self::Failed | Self::Cancelled
        )
    }
}

/// wait_agent 观察到的最小状态快照。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentWaitSnapshot {
    pub turn_presence: AgentTurnPresence,
    pub status: AgentLifecycleStatusKind,
}

impl AgentWaitSnapshot {
    /// 根据当前 turn presence 和状态分类判断 wait 是否可以返回。
    pub fn completion(self) -> AgentWaitCompletion {
        if matches!(self.turn_presence, AgentTurnPresence::NoActiveTurn)
            || self.status.is_wait_completion_status()
        {
            AgentWaitCompletion::Complete
        } else {
            AgentWaitCompletion::Pending
        }
    }

    pub fn is_complete(self) -> bool {
        matches!(self.completion(), AgentWaitCompletion::Complete)
    }
}

/// wait_agent 的通用完成判断结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentWaitCompletion {
    Complete,
    Pending,
}

/// 准备启动 agent turn 时观察到的最小状态快照。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentTurnStartSnapshot {
    pub status: AgentLifecycleStatusKind,
}

impl AgentTurnStartSnapshot {
    /// 判断当前状态是否允许启动新 turn。
    pub fn readiness(self) -> AgentTurnStartReadiness {
        if self.status.is_turn_start_ready() {
            AgentTurnStartReadiness::Ready
        } else {
            AgentTurnStartReadiness::Busy
        }
    }

    pub fn can_start(self) -> bool {
        matches!(self.readiness(), AgentTurnStartReadiness::Ready)
    }
}

/// 启动 agent turn 的共享可用性判断结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentTurnStartReadiness {
    Ready,
    Busy,
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

/// 子代理工具注册扩展点。
///
/// 宿主通过该 trait 把父 agent 的产品工具和共享工具 profile 传给子代理。
/// 实现方只负责注册工具，不应重新实现模型 turn loop、trace 或 agent 状态机。
pub trait AgentToolRegistrar: std::fmt::Debug + Send + Sync {
    fn register_tools<'a>(
        &'a self,
        core: &'a mut crate::PureCore,
        workspace_root: PathBuf,
        workspace_instructions: Option<String>,
    ) -> Pin<Box<dyn std::future::Future<Output = pl_protocol::Result<()>> + Send + 'a>>;
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
    pub tool_registrar: Option<Arc<dyn AgentToolRegistrar>>,
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
