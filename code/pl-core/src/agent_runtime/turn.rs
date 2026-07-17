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
        }
    }

    /// 设置 turn 完成后的 canonical session 提交策略。
    pub fn with_session_commit(mut self, policy: AgentSessionCommitPolicy) -> Self {
        self.session_commit = policy;
        self
    }

    pub(crate) fn with_runtime_context(
        mut self,
        turn_id: &TurnId,
        cancellation: CancellationToken,
    ) -> Self {
        self.request.turn_id = Some(turn_id.to_string());
        self.options.cancellation_token = Some(cancellation);
        self.options.execution_policy = Some(self.policy.clone());
        self
    }
}
