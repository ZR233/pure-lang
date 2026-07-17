use std::error::Error;
use std::future::Future;

use pl_trace::TraceEvent;

use super::{
    AgentDurableState, AgentId, AgentRuntimeEvent, AgentSnapshot, AgentTurnPreparationContext,
    PreparedAgentTurn,
};

/// runtime 启动时由 repository 返回的 durable agent。
#[derive(Debug, Clone)]
pub struct RestoredAgentRuntime {
    pub state: AgentDurableState,
}

/// repository 一次原子提交的完整输入。
///
/// 实现必须在同一个事务中校验 revision，并写入 snapshot、sessions、turn、queue、
/// durable events 与 traces。只有返回 `Applied` 后 runtime 才更新内存状态和广播事件。
#[derive(Debug, Clone)]
pub struct AgentCommit {
    pub agent_id: AgentId,
    pub expected_revision: Option<u64>,
    pub next_state: AgentDurableState,
    pub events: Vec<AgentRuntimeEvent>,
    pub trace_events: Vec<TraceEvent>,
}

/// repository CAS 提交结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentCommitOutcome {
    Applied,
    RevisionConflict { actual_revision: Option<u64> },
}

/// repository 已原子提交、可安全交给产品投影层的事件批次。
#[derive(Debug, Clone)]
pub struct AgentCommittedEvent {
    pub agent_id: AgentId,
    pub session_id: Option<super::SessionId>,
    pub turn_id: Option<super::TurnId>,
    pub runtime_events: Vec<AgentRuntimeEvent>,
    pub trace_events: Vec<TraceEvent>,
}

impl AgentCommittedEvent {
    pub(crate) fn runtime(event: AgentRuntimeEvent) -> Self {
        Self {
            agent_id: event.agent_id.clone(),
            session_id: None,
            turn_id: None,
            runtime_events: vec![event],
            trace_events: Vec::new(),
        }
    }
}

/// agent durable state 的产品存储端口。
///
/// 实现者负责事务和 expected revision CAS，不得在返回 `Applied` 前广播事件。
pub trait AgentStateRepository: Clone + Send + Sync + 'static {
    type Error: Error + Send + Sync + 'static;

    fn restore_runtime(
        &self,
    ) -> impl Future<Output = std::result::Result<Vec<RestoredAgentRuntime>, Self::Error>> + Send;

    fn commit(
        &self,
        commit: AgentCommit,
    ) -> impl Future<Output = std::result::Result<AgentCommitOutcome, Self::Error>> + Send;
}

/// 宿主为一次 turn 构造模型、instructions、工具和产品策略的端口。
///
/// 实现只准备值，不得启动任务或修改 agent 状态。
pub trait AgentTurnFactory: Clone + Send + Sync + 'static {
    type Error: Error + Send + Sync + 'static;

    fn prepare_turn(
        &self,
        context: AgentTurnPreparationContext,
    ) -> impl Future<Output = std::result::Result<PreparedAgentTurn, Self::Error>> + Send;
}

/// spawn 外部资源的准备上下文。
#[derive(Debug, Clone)]
pub struct SpawnLifecycleRequest {
    pub parent: AgentSnapshot,
    pub child: AgentSnapshot,
    pub metadata: serde_json::Value,
}

/// close 外部资源的准备上下文。
#[derive(Debug, Clone)]
pub struct CloseLifecycleRequest {
    pub agent: AgentSnapshot,
}

/// 产品容器、worktree 等外部资源的幂等 saga 端口。
///
/// prepare 返回可回滚 lease；activate/commit 成功后资源可见。rollback 必须允许重复调用，
/// 且不能删除不属于该 lease 的外部资源。
pub trait AgentLifecycleAdapter: Clone + Send + Sync + 'static {
    type Error: Error + Send + Sync + 'static;
    type SpawnLease: Send + 'static;
    type CloseLease: Send + 'static;

    fn prepare_spawn(
        &self,
        request: SpawnLifecycleRequest,
    ) -> impl Future<Output = std::result::Result<Self::SpawnLease, Self::Error>> + Send;

    fn activate_spawn(
        &self,
        lease: &Self::SpawnLease,
    ) -> impl Future<Output = std::result::Result<(), Self::Error>> + Send;

    fn rollback_spawn(
        &self,
        lease: Self::SpawnLease,
    ) -> impl Future<Output = std::result::Result<(), Self::Error>> + Send;

    fn prepare_close(
        &self,
        request: CloseLifecycleRequest,
    ) -> impl Future<Output = std::result::Result<Self::CloseLease, Self::Error>> + Send;

    fn commit_close(
        &self,
        lease: &Self::CloseLease,
    ) -> impl Future<Output = std::result::Result<(), Self::Error>> + Send;

    fn rollback_close(
        &self,
        lease: Self::CloseLease,
    ) -> impl Future<Output = std::result::Result<(), Self::Error>> + Send;
}

/// 已持久化 runtime event 的无失败广播端口。
///
/// 实现内部可记录广播错误，但不得把广播失败反馈为 durable transaction 失败。
pub trait AgentEventSink: Clone + Send + Sync + 'static {
    fn publish(&self, committed: AgentCommittedEvent) -> impl Future<Output = ()> + Send;
}

/// 产品接入 agent runtime 的 host bundle。
pub trait AgentRuntimeHost: Clone + Send + Sync + 'static {
    type Error: Error + Send + Sync + 'static;
    type Repository: AgentStateRepository<Error = Self::Error>;
    type TurnFactory: AgentTurnFactory<Error = Self::Error>;
    type Lifecycle: AgentLifecycleAdapter<Error = Self::Error>;
    type Events: AgentEventSink;

    fn repository(&self) -> &Self::Repository;
    fn turn_factory(&self) -> &Self::TurnFactory;
    fn lifecycle(&self) -> &Self::Lifecycle;
    fn events(&self) -> &Self::Events;
}
