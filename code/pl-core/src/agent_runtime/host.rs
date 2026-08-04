use std::error::Error;
use std::future::Future;

use pl_protocol::{SessionEventEnvelope, SessionViewSnapshot};
use pl_trace::TraceEvent;

use crate::ModelContextItem;

use super::{
    AgentDurableState, AgentId, AgentRuntimeEvent, AgentSnapshot, AgentTurnPreparationContext,
    PreparedAgentTurn,
};

/// runtime 启动时由 repository 返回的 durable agent。
#[derive(Debug, Clone)]
pub struct RestoredAgentRuntime {
    pub state: AgentDurableState,
    pub session_projection: Option<RestoredSessionProjection>,
}

/// 进程恢复时用于重建 session hub 的已提交 projection 与有界 journal。
#[derive(Debug, Clone)]
pub struct RestoredSessionProjection {
    pub snapshot: SessionViewSnapshot,
    pub retained_durable_events: Vec<SessionEventEnvelope>,
}

/// 一次会话历史事实与其可重建状态投影的完整提交。
///
/// 实现必须先持久化 `facts`，再校验 revision 并写入 snapshot、canonical session、turn、queue
/// 和 projection。只有返回 `Applied` 后 runtime 才更新内存状态和广播事件。
#[derive(Debug, Clone)]
pub struct SessionHistoryCommit {
    pub agent_id: AgentId,
    pub expected_revision: Option<u64>,
    pub next_state: AgentDurableState,
    pub facts: DurableCommitFacts,
    pub mutation: AgentStateMutation,
}

/// 一次提交中需要先于状态投影持久化的 typed durable facts。
#[derive(Debug, Clone)]
pub struct DurableCommitFacts {
    pub session_id: super::SessionId,
    pub turn_id: Option<super::TurnId>,
    pub through_sequence: u64,
    pub revision: u64,
    pub items: Vec<SessionEventEnvelope>,
    pub turn_transition: Option<pl_protocol::SessionTurn>,
    pub context: Option<SessionContextMutation>,
    pub projection_snapshot: Option<SessionViewSnapshot>,
    pub runtime_events: Vec<AgentRuntimeEvent>,
    pub trace_events: Vec<TraceEvent>,
}

impl DurableCommitFacts {
    pub fn from_state(
        state: &AgentDurableState,
        runtime_events: Vec<AgentRuntimeEvent>,
        trace_events: Vec<TraceEvent>,
        projection: Option<SessionProjectionCommit>,
        context: Option<SessionContextMutation>,
    ) -> Self {
        let items = projection
            .as_ref()
            .map(|projection| projection.durable_events.clone())
            .unwrap_or_default();
        let turn_transition = items.iter().rev().find_map(|event| match &event.kind {
            pl_protocol::SessionEventKind::TurnChanged { turn } => Some(turn.clone()),
            pl_protocol::SessionEventKind::MessageChanged { .. }
            | pl_protocol::SessionEventKind::MessageRemoved { .. }
            | pl_protocol::SessionEventKind::PartChanged { .. }
            | pl_protocol::SessionEventKind::PartRemoved { .. }
            | pl_protocol::SessionEventKind::PartDelta { .. }
            | pl_protocol::SessionEventKind::InteractionChanged { .. }
            | pl_protocol::SessionEventKind::AgentChanged { .. }
            | pl_protocol::SessionEventKind::TimelineEventAppended { .. }
            | pl_protocol::SessionEventKind::RuntimeChanged { .. }
            | pl_protocol::SessionEventKind::SkillActivated { .. }
            | pl_protocol::SessionEventKind::PlanChanged { .. }
            | pl_protocol::SessionEventKind::ContextCompacted { .. }
            | pl_protocol::SessionEventKind::ErrorOccurred { .. } => None,
        });
        let turn_id = turn_transition
            .as_ref()
            .and_then(|turn| super::TurnId::new(turn.turn_id.clone()).ok())
            .or_else(|| state.snapshot.active_turn_id.clone())
            .or_else(|| {
                state
                    .snapshot
                    .last_turn
                    .as_ref()
                    .map(|outcome| outcome.turn_id.clone())
            });
        Self {
            session_id: state.session.id.clone(),
            turn_id,
            through_sequence: state.session.session_event_sequence,
            revision: state.snapshot.revision,
            items,
            turn_transition,
            context,
            projection_snapshot: projection.map(|projection| projection.snapshot),
            runtime_events,
            trace_events,
        }
    }
}

/// checkpoint 对完整模型上下文的单调变更。
#[derive(Debug, Clone)]
pub enum SessionContextMutation {
    Append { items: Vec<ModelContextItem> },
    Replace { items: Vec<ModelContextItem> },
}

/// repository 与 session hub 共享的 canonical session 投影提交。
#[derive(Debug, Clone)]
pub struct SessionProjectionCommit {
    pub snapshot: SessionViewSnapshot,
    pub durable_events: Vec<SessionEventEnvelope>,
}

/// repository 可据此只更新真正变化的 durable aggregate 部分。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentStateMutation {
    SnapshotAndQueue,
    ReplaceSession { session_id: super::SessionId },
    AppendTrace,
    AppendSessionEvents { session_id: super::SessionId },
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
    pub session_events: Vec<SessionEventEnvelope>,
}

impl AgentCommittedEvent {
    pub(crate) fn runtime(event: AgentRuntimeEvent) -> Self {
        Self {
            agent_id: event.agent_id.clone(),
            session_id: None,
            turn_id: None,
            runtime_events: vec![event],
            trace_events: Vec::new(),
            session_events: Vec::new(),
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
        commit: SessionHistoryCommit,
    ) -> impl Future<Output = std::result::Result<AgentCommitOutcome, Self::Error>> + Send;

    /// 等待此前接受的历史事实和状态投影全部达到 durable watermark。
    fn barrier(&self) -> impl Future<Output = std::result::Result<(), Self::Error>> + Send;
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
    pub child_session_id: super::SessionId,
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
pub trait AgentCommitObserver: Clone + Send + Sync + 'static {
    fn publish(&self, committed: AgentCommittedEvent) -> impl Future<Output = ()> + Send;
}

/// 产品接入 agent runtime 的 host bundle。
pub trait AgentRuntimeHost: Clone + Send + Sync + 'static {
    type Error: Error + Send + Sync + 'static;
    type Repository: AgentStateRepository<Error = Self::Error>;
    type TurnFactory: AgentTurnFactory<Error = Self::Error>;
    type Lifecycle: AgentLifecycleAdapter<Error = Self::Error>;
    type Observer: AgentCommitObserver;

    fn repository(&self) -> &Self::Repository;
    fn turn_factory(&self) -> &Self::TurnFactory;
    fn lifecycle(&self) -> &Self::Lifecycle;
    fn observer(&self) -> &Self::Observer;
}
