use std::error::Error;
use std::future::Future;

use pl_protocol::{ThreadNotification, ThreadNotificationEnvelope, ThreadSnapshot, Turn};
use pl_trace::TraceEvent;

use crate::ModelContextItem;

use super::{
    AgentRuntimeEvent, AgentSnapshot, AgentTurnPreparationContext, PreparedAgentTurn,
    ThreadActorState, ThreadId,
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

/// Thread commit 的落库边界。
///
/// 内存 snapshot 是唯一权威实例；`Batched` 只入队、由后台 writer 批量落库，
/// `Immediate` 额外等待包含该 commit 的批量事务完成后才返回。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitDurability {
    Batched,
    Immediate,
}

impl CommitDurability {
    /// 按 runtime 事件类型推导默认落库边界。
    ///
    /// 输入队列、Turn 终态、注册与故障是 durable 边界；活动投影与普通状态
    /// 变化属于流式增量。调用点可用显式标记覆盖默认值。
    pub fn for_event(kind: &super::AgentRuntimeEventKind) -> Self {
        match kind {
            super::AgentRuntimeEventKind::Registered { .. }
            | super::AgentRuntimeEventKind::TurnQueued { .. }
            | super::AgentRuntimeEventKind::TurnStarted { .. }
            | super::AgentRuntimeEventKind::ThreadOpened { .. }
            | super::AgentRuntimeEventKind::TurnFinished { .. }
            | super::AgentRuntimeEventKind::RecoveryCancelledTurn { .. }
            | super::AgentRuntimeEventKind::Faulted { .. } => Self::Immediate,
            super::AgentRuntimeEventKind::StateChanged { .. }
            | super::AgentRuntimeEventKind::TurnActivityChanged { .. } => Self::Batched,
        }
    }
}

/// ThreadActor 的一次原子提交。
///
/// 实现先把 commit 写入 write-behind 队列并按 [`CommitDurability`] 决定是否等待
/// flush；内存 state 在 commit 返回 `Applied` 后由 runtime 更新并广播事件。
#[derive(Debug, Clone)]
pub struct ThreadCommit {
    pub agent_id: ThreadId,
    pub durability: CommitDurability,
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
    pub inference: Option<super::AgentInferenceCommit>,
    /// `report_progress` 触发时追加到 `thread_submissions` 的阶段提交记录。
    #[allow(clippy::struct_field_names)]
    pub submission: Option<super::ProgressSubmissionCommit>,
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
            inference: None,
            submission: None,
        }
    }
}

/// checkpoint 对 append-only transcript 的单调变更。
#[derive(Debug, Clone)]
pub enum ThreadContextMutation {
    /// `items` 只包含相对于已持久 transcript 的新 suffix。
    Append { items: Vec<ModelContextItem> },
    /// `items` 是压缩、回滚或截断后的完整新 transcript。
    Replace { items: Vec<ModelContextItem> },
}

/// 根据提交前后的 canonical transcript 派生唯一持久化 mutation。
///
/// 所有 Thread transition 和初始注册都必须复用该函数，避免 actor 已替换
/// session、repository 却仍保留旧 baseline。
pub(crate) fn transcript_mutation(
    previous: &[ModelContextItem],
    next: &[ModelContextItem],
) -> Option<ThreadContextMutation> {
    if previous == next {
        return None;
    }
    if let Some(suffix) = next.strip_prefix(previous) {
        return Some(ThreadContextMutation::Append {
            items: suffix.to_vec(),
        });
    }
    Some(ThreadContextMutation::Replace {
        items: next.to_vec(),
    })
}

/// 新 Thread 没有可追加的 durable baseline，非空历史必须以 replacement 起始。
pub(crate) fn initial_transcript_mutation(
    items: &[ModelContextItem],
) -> Option<ThreadContextMutation> {
    (!items.is_empty()).then(|| ThreadContextMutation::Replace {
        items: items.to_vec(),
    })
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
    pub agent_id: ThreadId,
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
/// 内存 snapshot 是唯一权威实例：`commit` 先入队 write-behind writer，按
/// `commit.durability` 决定是否等待 flush 完成后才返回 `Applied`；`flush_pending`
/// 与 `pending_commit_count` 供淘汰和关机等待积压落库。
pub trait ThreadRepository: Clone + Send + Sync + 'static {
    type Error: Error + Send + Sync + 'static;

    fn restore_runtime(
        &self,
    ) -> impl Future<Output = std::result::Result<Vec<RestoredAgentRuntime>, Self::Error>> + Send;

    /// 按需恢复单个 Thread 的 durable runtime（惰性驻留）。
    ///
    /// `restore_runtime` 只返回启动钉住集合；未驻留 Thread 在订阅、提交输入
    /// 或修复时通过本方法恢复。Thread 不存在或尚未注册 runtime 时返回 `None`。
    fn restore_thread(
        &self,
        thread_id: &super::ThreadId,
    ) -> impl Future<Output = std::result::Result<Option<RestoredAgentRuntime>, Self::Error>> + Send;

    fn commit(
        &self,
        commit: ThreadCommit,
    ) -> impl Future<Output = std::result::Result<ThreadCommitOutcome, Self::Error>> + Send;

    /// 等待当前全部（或指定 Thread 的）pending commit 完成落库。
    ///
    /// LRU 淘汰与关机必须在 drop actor 之前调用；writer 已停止或存在不可恢复
    /// 落库失败时返回错误。
    fn flush_pending(
        &self,
        thread_id: Option<&super::ThreadId>,
    ) -> impl Future<Output = std::result::Result<(), Self::Error>> + Send;

    /// 当前尚未落库的 pending commit 数量，用于关机进度。
    fn pending_commit_count(&self) -> usize;

    /// 读取某 agent 的 durable 阶段提交历史（含已关闭 agent）。
    ///
    /// 用于主代理主动 pull 子代理报告：覆盖全状态、按提交顺序分页、不截断。
    fn list_submissions(
        &self,
        thread_id: &super::ThreadId,
        offset: usize,
        limit: usize,
    ) -> impl Future<Output = std::result::Result<super::AgentSubmissionPage, Self::Error>> + Send;
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

    /// 返回必须与 child 初始 runtime snapshot 一起持久化的产品上下文。
    fn initial_context(
        &self,
        _lease: &Self::SpawnLease,
    ) -> std::result::Result<Vec<crate::PinnedContextSection>, Self::Error> {
        Ok(Vec::new())
    }

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
