use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio_util::sync::CancellationToken;

use crate::{AgentKernel, AgentSession, TurnOptions, TurnRequest};

use super::{
    AgentExecutionPolicy, AgentRuntimeHandle, AgentSnapshot, PendingAgentInput, SessionId, TurnId,
};

/// turn 完成后对 canonical session 的提交策略。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AgentSessionCommitPolicy {
    /// 提交本轮产生的用户、模型和工具上下文。
    #[default]
    Persist,
    /// 仅提交 outcome、usage 和 trace，丢弃本轮对 session context 的修改。
    DiscardTurn,
}

/// 宿主准备一次 turn 时可读取的稳定上下文。
#[derive(Debug, Clone)]
pub struct AgentTurnPreparationContext {
    pub snapshot: AgentSnapshot,
    pub turn_id: TurnId,
    pub session_id: SessionId,
    pub input: PendingAgentInput,
    pub session: AgentSession,
    pub trace_sequence: u64,
    pub runtime: AgentRuntimeHandle,
    pub cancellation_token: CancellationToken,
}

/// 宿主为 runtime 准备好的可执行 turn。
#[derive(Debug)]
pub struct PreparedAgentTurn {
    pub(crate) kernel: AgentKernel,
    pub(crate) request: TurnRequest,
    pub(crate) options: TurnOptions,
    pub(crate) policy: AgentExecutionPolicy,
    pub(crate) session_commit: AgentSessionCommitPolicy,
    pub(crate) pinned_context: Vec<crate::PinnedContextSection>,
}

impl PreparedAgentTurn {
    /// 创建 prepared turn；runtime 会覆盖 turn id 与 cancellation token。
    pub fn new(
        kernel: AgentKernel,
        request: TurnRequest,
        options: TurnOptions,
        policy: AgentExecutionPolicy,
    ) -> Self {
        Self {
            kernel,
            request,
            options,
            policy,
            session_commit: AgentSessionCommitPolicy::Persist,
            pinned_context: Vec::new(),
        }
    }

    /// 设置 turn 完成后的 canonical session 提交策略。
    pub fn with_session_commit(mut self, policy: AgentSessionCommitPolicy) -> Self {
        self.session_commit = policy;
        self
    }

    /// 在模型 turn 启动前写入产品提供的 canonical pinned context。
    pub fn with_pinned_context(mut self, section: crate::PinnedContextSection) -> Self {
        self.pinned_context.push(section);
        self
    }

    pub(crate) fn with_runtime_context(
        mut self,
        turn_id: &TurnId,
        cancellation: CancellationToken,
        checkpoint: AgentTurnCheckpointHandle,
    ) -> Self {
        self.request.turn_id = Some(turn_id.to_string());
        self.options.cancellation_token = Some(cancellation);
        self.options.execution_policy = Some(self.policy.clone());
        self.options.checkpoint = Some(checkpoint);
        self
    }
}

/// mid-turn durable session checkpoint 的触发原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnCheckpointReason {
    WorkingSetChanged,
    BeforeInference,
    ContextCompacted,
    Terminal,
}

/// worker 交给 actor 做 active-turn 与 sequence 校验的 session checkpoint。
#[derive(Debug, Clone)]
pub struct AgentTurnCheckpoint {
    pub turn_id: TurnId,
    pub session_id: SessionId,
    pub sequence: u64,
    pub session: AgentSession,
    pub reason: TurnCheckpointReason,
}

/// TurnEngine 使用的 durable checkpoint 命令句柄。
#[derive(Clone)]
pub struct AgentTurnCheckpointHandle {
    runtime: AgentRuntimeHandle,
    agent_id: super::AgentId,
    turn_id: TurnId,
    session_id: SessionId,
    sequence: Arc<AtomicU64>,
}

impl AgentTurnCheckpointHandle {
    pub(crate) fn new(
        runtime: AgentRuntimeHandle,
        agent_id: super::AgentId,
        turn_id: TurnId,
        session_id: SessionId,
    ) -> Self {
        Self {
            runtime,
            agent_id,
            turn_id,
            session_id,
            sequence: Arc::new(AtomicU64::new(0)),
        }
    }

    pub async fn checkpoint(
        &self,
        session: AgentSession,
        reason: TurnCheckpointReason,
    ) -> super::AgentRuntimeResult<()> {
        let sequence = self
            .sequence
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        self.runtime
            .checkpoint_turn(
                self.agent_id.clone(),
                AgentTurnCheckpoint {
                    turn_id: self.turn_id.clone(),
                    session_id: self.session_id.clone(),
                    sequence,
                    session,
                    reason,
                },
            )
            .await
    }
}

impl std::fmt::Debug for AgentTurnCheckpointHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AgentTurnCheckpointHandle")
            .field("agent_id", &self.agent_id)
            .field("turn_id", &self.turn_id)
            .field("session_id", &self.session_id)
            .finish_non_exhaustive()
    }
}
