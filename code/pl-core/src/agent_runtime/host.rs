use std::error::Error;
use std::future::Future;

use pl_protocol::{ThreadNotification, ThreadNotificationEnvelope, ThreadSnapshot, Turn};
use pl_trace::TraceEvent;

use crate::ModelContextItem;

use super::{
    AgentId, AgentRuntimeEvent, AgentSnapshot, AgentTurnPreparationContext, PreparedAgentTurn,
    ThreadActorState,
};

/// runtime 启动时由 repository 返回的 durable agent。
#[derive(Debug, Clone)]
pub struct RestoredAgentRuntime {
    pub state: ThreadActorState,
    pub thread_snapshot: Option<RestoredThreadSnapshot>,
}

/// 进程恢复时用于初始化 Thread 订阅的 authoritative snapshot。
#[derive(Debug, Clone)]
pub struct RestoredThreadSnapshot {
    pub snapshot: ThreadSnapshot,
}

/// ThreadActor 的一次原子提交。
///
/// 实现必须在单个事务内校验 revision 并写入涉及的 Thread、Turn、Item、Input、Interaction
/// 与模型上下文。只有返回 `Applied` 后 runtime 才更新内存状态和广播事件。
#[derive(Debug, Clone)]
pub struct ThreadCommit {
    pub agent_id: AgentId,
    pub expected_revision: Option<u64>,
    pub next_state: ThreadActorState,
    pub facts: DurableCommitFacts,
    pub mutation: ThreadMutation,
}

/// 一次提交中需要原子持久化的 typed Thread 变更。
#[derive(Debug, Clone)]
pub struct DurableCommitFacts {
    pub thread_id: super::ThreadId,
    pub turn_id: Option<super::TurnId>,
    pub through_revision: u64,
    pub revision: u64,
    pub notifications: Vec<ThreadNotificationEnvelope>,
    pub turn_transition: Option<Turn>,
    pub context: Option<ThreadContextMutation>,
    pub projection_snapshot: Option<ThreadSnapshot>,
    pub runtime_events: Vec<AgentRuntimeEvent>,
    pub trace_events: Vec<TraceEvent>,
}

impl DurableCommitFacts {
    pub fn from_state(
        state: &ThreadActorState,
        runtime_events: Vec<AgentRuntimeEvent>,
        trace_events: Vec<TraceEvent>,
        projection: Option<ThreadProjectionCommit>,
        context: Option<ThreadContextMutation>,
    ) -> Self {
        let notifications = projection
            .as_ref()
            .map(|projection| projection.notifications.clone())
            .unwrap_or_default();
        let turn_transition =
            notifications
                .iter()
                .rev()
                .find_map(|event| match &event.notification {
                    ThreadNotification::TurnStarted { turn }
                    | ThreadNotification::TurnUpdated { turn }
                    | ThreadNotification::TurnCompleted { turn } => Some(turn.clone()),
                    ThreadNotification::ItemStarted { .. }
                    | ThreadNotification::ItemDelta { .. }
                    | ThreadNotification::ItemCompleted { .. }
                    | ThreadNotification::InteractionChanged { .. }
                    | ThreadNotification::ThreadRuntimeUpdated { .. }
                    | ThreadNotification::Lagged { .. } => None,
                });
        let turn_id = turn_transition
            .as_ref()
            .and_then(|turn| super::TurnId::new(turn.id.clone()).ok())
            .or_else(|| state.snapshot.active_turn_id.clone())
            .or_else(|| {
                state
                    .snapshot
                    .last_turn
                    .as_ref()
                    .map(|outcome| outcome.turn_id.clone())
            });
        Self {
            thread_id: state.snapshot.identity.id.clone(),
            turn_id,
            through_revision: state.session.thread_revision,
            revision: state.snapshot.revision,
            notifications,
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
pub enum ThreadContextMutation {
    Append { items: Vec<ModelContextItem> },
    Replace { items: Vec<ModelContextItem> },
}

/// Thread snapshot 与同一 revision 的实时通知。
#[derive(Debug, Clone)]
pub struct ThreadProjectionCommit {
    pub snapshot: ThreadSnapshot,
    pub notifications: Vec<ThreadNotificationEnvelope>,
}

/// repository 可据此只更新真正变化的 durable aggregate 部分。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThreadMutation {
    SnapshotAndQueue,
    ReplaceThread { thread_id: super::ThreadId },
    AppendTrace,
    AppendThreadNotifications { thread_id: super::ThreadId },
}

/// repository CAS 提交结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadCommitOutcome {
    Applied,
    RevisionConflict { actual_revision: Option<u64> },
}

/// repository 已原子提交、可安全发布的事件批次。
#[derive(Debug, Clone)]
pub struct AgentCommittedEvent {
    pub agent_id: AgentId,
    pub thread_id: Option<super::ThreadId>,
    pub turn_id: Option<super::TurnId>,
    pub runtime_events: Vec<AgentRuntimeEvent>,
    pub trace_events: Vec<TraceEvent>,
    pub thread_notifications: Vec<ThreadNotificationEnvelope>,
}

impl AgentCommittedEvent {
    pub(crate) fn runtime(event: AgentRuntimeEvent) -> Self {
        Self {
            agent_id: event.agent_id.clone(),
            thread_id: None,
            turn_id: None,
            runtime_events: vec![event],
            trace_events: Vec::new(),
            thread_notifications: Vec::new(),
        }
    }
}

/// ThreadActor 使用的 canonical 存储端口。
///
/// 实现者负责事务和 expected revision CAS，不得在返回 `Applied` 前广播事件。
pub trait ThreadRepository: Clone + Send + Sync + 'static {
    type Error: Error + Send + Sync + 'static;

    fn restore_runtime(
        &self,
    ) -> impl Future<Output = std::result::Result<Vec<RestoredAgentRuntime>, Self::Error>> + Send;

    fn commit(
        &self,
        commit: ThreadCommit,
    ) -> impl Future<Output = std::result::Result<ThreadCommitOutcome, Self::Error>> + Send;
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
    pub child_thread_id: super::ThreadId,
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
    type Repository: ThreadRepository<Error = Self::Error>;
    type TurnFactory: AgentTurnFactory<Error = Self::Error>;
    type Lifecycle: AgentLifecycleAdapter<Error = Self::Error>;
    type Observer: AgentCommitObserver;

    fn repository(&self) -> &Self::Repository;
    fn turn_factory(&self) -> &Self::TurnFactory;
    fn lifecycle(&self) -> &Self::Lifecycle;
    fn observer(&self) -> &Self::Observer;
}
