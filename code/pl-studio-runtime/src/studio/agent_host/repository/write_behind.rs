//! Thread commit 的 write-behind 批量落库 writer。
//!
//! 内存 snapshot 是唯一权威实例；commit 进入本进程内队列后即可发布，后台 task
//! 按 FIFO 分批在单个 SQLite 事务中应用。瞬时错误永久保留批次并自动退避重试；
//! 修订冲突等不变量错误进入 Blocked，但不会删除任何待落库事实。

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pl_core::{PersistenceClass, ThreadCommit};
use pl_protocol::StateError;
use sea_orm::TransactionTrait;
use tokio::sync::{Notify, oneshot, watch};
use tokio::task::JoinHandle;

use crate::PureError;
use crate::studio::runtime::{MODEL_PERFORMANCE_OWNER_ID, ModelPerformanceState};
use crate::studio::store::directory::{DirectoryDelta, apply_directory_delta};
use crate::studio::store::object::put_object;
use crate::studio::{
    BlockedPersistence, DegradedPersistence, FlushingPersistence, PersistenceState,
    PersistenceStateSnapshot, ReadyPersistence, RecoveringPersistence, StudioStore, unix_seconds,
};

use super::{ApplyCommitOutcome, apply_state_commit, store_error};

/// 单批最多应用的 commit 数；一批共享一个 SQLite 事务。
const MAX_BATCH_COMMITS: usize = 64;
/// 普通提交上限；其后的容量只供终态收束使用。
const NORMAL_PENDING_COMMITS: usize = 768;
/// 总积压上限；达到后入队方等待 writer 追赶（背压而不是丢弃）。
const MAX_PENDING_COMMITS: usize = 1024;
/// 首条待写事实允许等待的最大批量时间窗口。
const FLUSH_INTERVAL: Duration = Duration::from_secs(5);
/// 进入公开 Degraded 状态前的快速重试次数。
const FAST_BATCH_RETRIES: usize = 3;
/// 瞬时失败的重试退避基值。
const RETRY_BACKOFF: Duration = Duration::from_millis(100);
/// Degraded 后的最大自动重试间隔。
const MAX_RETRY_BACKOFF: Duration = Duration::from_secs(30);

/// 穷尽的 Studio typed persistence mutation。
///
/// Thread 包含 working object、Transcript、Timeline 与 Thread projection；
/// Directory 包含产品目录和其他有界 Studio object。
#[derive(Debug, Clone)]
enum StudioMutation {
    Thread(Box<ThreadCommit>),
    Directory(Box<StudioDirectoryMutation>),
}

#[derive(Debug, Clone)]
enum StudioDirectoryMutation {
    Delta(DirectoryDelta),
    ModelPerformance(ObservedStateCommit),
}

#[derive(Debug, Clone)]
struct QueuedMutation {
    accepted_at: tokio::time::Instant,
    mutation: StudioMutation,
}

impl QueuedMutation {
    fn new(mutation: StudioMutation) -> Self {
        Self {
            accepted_at: tokio::time::Instant::now(),
            mutation,
        }
    }
}

enum QueueEntry {
    Mutation(QueuedMutation),
    Barrier(oneshot::Sender<Result<(), PureError>>),
}

#[derive(Debug, Clone)]
struct ObservedStateCommit {
    revision: u64,
    value: ModelPerformanceState,
}

impl QueueEntry {
    const fn is_commit(&self) -> bool {
        matches!(self, Self::Mutation(_))
    }

    fn terminal_key(&self) -> Option<String> {
        match self {
            Self::Mutation(QueuedMutation {
                mutation: StudioMutation::Thread(commit),
                ..
            }) => terminal_turn_key(commit),
            Self::Mutation(_) | Self::Barrier(_) => None,
        }
    }

    /// worker panic 恢复只复制 typed mutation；barrier 由失败路径显式唤醒。
    fn clone_commit(&self) -> Option<Self> {
        match self {
            Self::Mutation(commit) => Some(Self::Mutation(commit.clone())),
            Self::Barrier(_) => None,
        }
    }

    fn flushes_immediately(&self) -> bool {
        match self {
            Self::Mutation(QueuedMutation {
                mutation: StudioMutation::Thread(commit),
                ..
            }) => {
                commit.persistence == PersistenceClass::Settlement
                    || matches!(
                        commit.facts.context.as_ref(),
                        Some(pl_core::ThreadContextMutation::Replace { .. })
                    )
            }
            Self::Mutation(QueuedMutation {
                mutation: StudioMutation::Directory(_),
                ..
            }) => false,
            Self::Barrier(_) => true,
        }
    }

    fn accepted_at(&self) -> Option<tokio::time::Instant> {
        match self {
            Self::Mutation(commit) => Some(commit.accepted_at),
            Self::Barrier(_) => None,
        }
    }

    fn contains_directory_fact_for(&self, owner_id: &str) -> bool {
        matches!(
            self,
            Self::Mutation(QueuedMutation {
                mutation: StudioMutation::Directory(directory),
                ..
            }) if matches!(
                directory.as_ref(),
                StudioDirectoryMutation::Delta(delta) if delta.touches_thread(owner_id)
            )
        )
    }
}

fn queue_thread(commit: ThreadCommit) -> QueueEntry {
    QueueEntry::Mutation(QueuedMutation::new(StudioMutation::Thread(Box::new(
        commit,
    ))))
}

fn queue_directory(delta: DirectoryDelta) -> QueueEntry {
    QueueEntry::Mutation(QueuedMutation::new(StudioMutation::Directory(Box::new(
        StudioDirectoryMutation::Delta(delta),
    ))))
}

fn queue_model_performance(commit: ObservedStateCommit) -> QueueEntry {
    QueueEntry::Mutation(QueuedMutation::new(StudioMutation::Directory(Box::new(
        StudioDirectoryMutation::ModelPerformance(commit),
    ))))
}

struct WriterShared {
    store: StudioStore,
    queue: Mutex<VecDeque<QueueEntry>>,
    /// 已从 queue 取出、但尚未 durable 的 typed mutation 副本。
    ///
    /// worker panic 时 supervisor 把它原序放回 queue；热状态从不依赖 worker
    /// 局部变量保存唯一一份待写事实。
    inflight: Mutex<VecDeque<QueueEntry>>,
    /// 入队方唤醒 writer。
    work_notify: Notify,
    /// writer 每次成功排空后发布进度，背压入队方据此重试。
    progress: watch::Sender<u64>,
    /// 任一 owner 的耐久修订推进时发布，供精确屏障等待。
    durable_progress: watch::Sender<u64>,
    durable_revisions: Mutex<HashMap<String, u64>>,
    settlement_slots: Mutex<SettlementSlots>,
    state: watch::Sender<PersistenceStateSnapshot>,
    retry_notify: Notify,
    stopping: AtomicBool,
}

#[derive(Default)]
struct SettlementSlots {
    active_turns: HashSet<String>,
    /// 已进入队列、但尚未确认落库的终态生命周期。
    ///
    /// 同一生命周期的重复终态必须等待前一条落库，不能重复消费预留许可。
    pending_terminal_turns: HashSet<String>,
}

/// write-behind 队列与后台 writer task 的共享句柄。
///
/// task 惰性启动：第一次 enqueue/flush 时创建；`shutdown` 排空队列并等待
/// task 退出。clone 共享同一队列与 task。
#[derive(Clone)]
pub(in crate::studio) struct ThreadWriteBehindWriter {
    shared: Arc<WriterShared>,
    task: Arc<Mutex<Option<JoinHandle<()>>>>,
    pending_commits: Arc<AtomicUsize>,
}

impl ThreadWriteBehindWriter {
    pub(in crate::studio) fn new(store: StudioStore) -> Self {
        let (progress, _) = watch::channel(0u64);
        let (durable_progress, _) = watch::channel(0u64);
        let (state, _) = watch::channel(PersistenceStateSnapshot::default());
        Self {
            shared: Arc::new(WriterShared {
                store,
                queue: Mutex::new(VecDeque::new()),
                inflight: Mutex::new(VecDeque::new()),
                work_notify: Notify::new(),
                progress,
                durable_progress,
                durable_revisions: Mutex::new(HashMap::new()),
                settlement_slots: Mutex::new(SettlementSlots::default()),
                state,
                retry_notify: Notify::new(),
                stopping: AtomicBool::new(false),
            }),
            task: Arc::new(Mutex::new(None)),
            pending_commits: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// 当前尚未落库的 pending commit 数。
    pub(in crate::studio) fn pending_commit_count(&self) -> usize {
        self.pending_commits.load(Ordering::Acquire)
    }

    pub(in crate::studio) fn state_snapshot(&self) -> PersistenceStateSnapshot {
        self.shared.state.borrow().clone()
    }

    /// 返回指定 owner 在本进程已确认写入 SQLite 的最高修订号。
    pub(in crate::studio) fn durable_revision(&self, owner_id: &str) -> Option<u64> {
        self.shared
            .durable_revisions
            .lock()
            .expect("durable revision lock poisoned")
            .get(owner_id)
            .copied()
    }

    /// 记录从 SQLite 恢复出的耐久基线；只允许单调推进。
    pub(in crate::studio) fn seed_durable_revision(&self, owner_id: &str, revision: u64) {
        advance_durable_revision(&self.shared, owner_id, revision);
    }

    pub(in crate::studio) fn subscribe_state(&self) -> watch::Receiver<PersistenceStateSnapshot> {
        self.shared.state.subscribe()
    }

    /// 跳过当前退避等待并立即重试队首批次。
    pub(in crate::studio) fn retry_now(&self) {
        let (sender, receiver) = oneshot::channel();
        drop(receiver);
        self.shared
            .queue
            .lock()
            .expect("write-behind queue lock poisoned")
            .push_back(QueueEntry::Barrier(sender));
        self.shared.retry_notify.notify_waiters();
        self.shared.work_notify.notify_one();
    }

    pub(in crate::studio) fn block(&self, reason: &str) {
        publish_blocked(&self.shared, reason);
    }

    /// 对 runtime owner 施加容量背压，直到同一份 typed commit 被原子接受。
    pub(in crate::studio) async fn accept_thread_with_backpressure(
        &self,
        commit: ThreadCommit,
    ) -> Result<(), PureError> {
        loop {
            let mut progress = self.shared.progress.subscribe();
            if self.try_accept_thread(&commit)? {
                return Ok(());
            }
            self.blocked_result()?;
            progress.changed().await.map_err(|_| {
                store_error("write-behind writer stopped while waiting for Thread capacity")
            })?;
        }
    }

    fn try_accept_thread(&self, commit: &ThreadCommit) -> Result<bool, PureError> {
        self.check_accepting()?;
        self.ensure_task();
        if commit.persistence == PersistenceClass::Coalescible {
            let mut queue = self.lock_queue()?;
            self.check_accepting()?;
            if try_coalesce_tail(&mut queue, commit.clone()) {
                drop(queue);
                self.shared.work_notify.notify_one();
                return Ok(true);
            }
        }
        if !self.try_enqueue_now(commit)? {
            return Ok(false);
        }
        update_healthy_state(&self.shared, self.pending_commits.load(Ordering::Acquire));
        self.shared.work_notify.notify_one();
        Ok(true)
    }

    /// 原子接受目录事实；调用方只有成功后才能更新热目录。
    pub(in crate::studio) fn accept_directory(
        &self,
        delta: DirectoryDelta,
    ) -> Result<(), PureError> {
        if delta.is_empty() {
            return Err(store_error("directory delta must carry at least one fact"));
        }
        self.check_accepting()?;
        self.ensure_task();
        if !self.try_enqueue_directory_now(&delta)? {
            return Err(store_error(
                "write-behind directory capacity is full; retry the command after persistence advances",
            ));
        }
        update_healthy_state(&self.shared, self.pending_commits.load(Ordering::Acquire));
        self.shared.work_notify.notify_one();
        Ok(())
    }

    /// 把模型性能 owner 的版本化 typed snapshot 送入同一 write-behind 队列。
    ///
    /// 尚未落库的旧 revision 会被最新完整值覆盖；此处不执行 serde。
    pub(in crate::studio) fn accept_model_performance(
        &self,
        value: ModelPerformanceState,
    ) -> Result<(), PureError> {
        self.check_accepting()?;
        self.ensure_task();
        let commit = ObservedStateCommit {
            revision: value.revision(),
            value,
        };
        let mut queue = self.lock_queue()?;
        self.check_accepting()?;
        if try_coalesce_observed_state(&mut queue, &commit) {
            drop(queue);
            self.shared.work_notify.notify_one();
            return Ok(());
        }
        let ordinary = queue
            .iter()
            .filter(|entry| entry.is_commit() && entry.terminal_key().is_none())
            .count();
        if ordinary >= NORMAL_PENDING_COMMITS {
            return Err(store_error(
                "write-behind model performance capacity is full; retry after persistence advances",
            ));
        }
        queue.push_back(queue_model_performance(commit));
        self.record_visible_commit();
        drop(queue);
        update_healthy_state(&self.shared, self.pending_commits.load(Ordering::Acquire));
        self.shared.work_notify.notify_one();
        Ok(())
    }

    /// 目录 delta 占用普通容量；队满时对调用方施加背压。
    fn try_enqueue_directory_now(&self, delta: &DirectoryDelta) -> Result<bool, PureError> {
        let mut queue = self.lock_queue()?;
        self.check_accepting()?;
        let ordinary = queue
            .iter()
            .filter(|entry| entry.is_commit() && entry.terminal_key().is_none())
            .count();
        if ordinary >= NORMAL_PENDING_COMMITS {
            return Ok(false);
        }
        queue.push_back(queue_directory(delta.clone()));
        self.record_visible_commit();
        Ok(true)
    }

    /// 等待一个 owner 的指定修订号被 SQLite 确认。
    pub(in crate::studio) async fn await_durable(
        &self,
        owner_id: &str,
        revision: u64,
    ) -> Result<(), PureError> {
        loop {
            if self
                .durable_revision(owner_id)
                .is_some_and(|durable| durable >= revision)
                && !self.has_pending_directory_fact(owner_id)?
            {
                return Ok(());
            }
            self.blocked_result()?;
            if self.shared.stopping.load(Ordering::Acquire) && self.task_is_none() {
                return Err(store_error(format!(
                    "write-behind writer stopped before owner {owner_id} revision {revision} became durable"
                )));
            }
            self.flush().await?;
        }
    }

    fn has_pending_directory_fact(&self, owner_id: &str) -> Result<bool, PureError> {
        let queued = self
            .shared
            .queue
            .lock()
            .map_err(|_| store_error("write-behind queue lock poisoned"))?
            .iter()
            .any(|entry| entry.contains_directory_fact_for(owner_id));
        if queued {
            return Ok(true);
        }
        Ok(self
            .shared
            .inflight
            .lock()
            .map_err(|_| store_error("write-behind inflight lock poisoned"))?
            .iter()
            .any(|entry| entry.contains_directory_fact_for(owner_id)))
    }

    /// 持久化已进入 Blocked 时，flush 立即返回诊断；瞬时故障则继续等待自动恢复。
    fn blocked_result(&self) -> Result<(), PureError> {
        match &self.state_snapshot().state {
            PersistenceState::Blocked(state) => Err(store_error(format!(
                "write-behind writer blocked: {}",
                state.error.message
            ))),
            PersistenceState::Ready(_)
            | PersistenceState::Flushing(_)
            | PersistenceState::Degraded(_)
            | PersistenceState::Recovering(_) => Ok(()),
        }
    }

    /// 等待当前队列中全部（含指定 Thread 的）pending commit 完成落库。
    pub(in crate::studio) async fn flush(&self) -> Result<(), PureError> {
        self.blocked_result()?;
        if self.shared.stopping.load(Ordering::Acquire) {
            return self.await_stopping_drain().await;
        }
        if self.task_is_none() {
            return Ok(());
        }
        let (sender, receiver) = oneshot::channel();
        {
            let mut queue = self.lock_queue()?;
            queue.push_back(QueueEntry::Barrier(sender));
        }
        self.shared.work_notify.notify_one();
        receiver
            .await
            .map_err(|_| store_error("write-behind writer dropped a flush barrier"))?
    }

    /// 排空队列并停止 writer task。瞬时故障会继续等待自动恢复。
    pub(in crate::studio) async fn shutdown(&self) -> Result<(), PureError> {
        self.shared.stopping.store(true, Ordering::Release);
        self.shared.work_notify.notify_one();
        self.shared.retry_notify.notify_waiters();
        let task = self.task.lock().expect("writer task lock").take();
        if let Some(task) = task {
            task.await
                .map_err(|error| store_error(format!("write-behind supervisor failed: {error}")))?;
        }
        self.blocked_result()?;
        if self.pending_commit_count() != 0 {
            return Err(store_error(format!(
                "write-behind writer stopped with {} pending commits",
                self.pending_commit_count()
            )));
        }
        Ok(())
    }

    async fn await_stopping_drain(&self) -> Result<(), PureError> {
        let mut progress = self.shared.durable_progress.subscribe();
        loop {
            self.blocked_result()?;
            if self.pending_commit_count() == 0 {
                return Ok(());
            }
            progress
                .changed()
                .await
                .map_err(|_| store_error("write-behind shutdown progress channel closed"))?;
        }
    }

    fn lock_queue(&self) -> Result<std::sync::MutexGuard<'_, VecDeque<QueueEntry>>, PureError> {
        self.shared
            .queue
            .lock()
            .map_err(|_| store_error("write-behind queue lock poisoned"))
    }

    fn check_accepting(&self) -> Result<(), PureError> {
        self.blocked_result()?;
        if self.shared.stopping.load(Ordering::Acquire) {
            return Err(store_error(
                "write-behind writer is shutting down and no longer accepts commits",
            ));
        }
        Ok(())
    }

    fn task_is_none(&self) -> bool {
        self.task.lock().expect("writer task lock").is_none()
    }

    fn ensure_task(&self) {
        let mut task = self.task.lock().expect("writer task lock");
        if task.is_none() {
            let shared = self.shared.clone();
            let pending = self.pending_commits.clone();
            *task = Some(tokio::spawn(supervise_writer(shared, pending)));
        }
    }

    fn try_enqueue_now(&self, commit: &ThreadCommit) -> Result<bool, PureError> {
        let mut queue = self.lock_queue()?;
        self.check_accepting()?;
        if let Some(terminal_key) = terminal_turn_key(commit) {
            let mut slots = self
                .shared
                .settlement_slots
                .lock()
                .map_err(|_| store_error("settlement slot lock poisoned"))?;
            if slots.pending_terminal_turns.contains(&terminal_key) {
                // 同一 Turn 的前一条终态仍在队列中。等待它落库并释放许可后再重试，
                // 保证重复终态不会把 256 个预留位置耗尽。
                return Ok(false);
            }
            let consumed_active = slots.active_turns.remove(&terminal_key);
            if !consumed_active
                && slots.active_turns.len() + slots.pending_terminal_turns.len()
                    >= MAX_PENDING_COMMITS - NORMAL_PENDING_COMMITS
            {
                // 恢复取消等终态可能没有在本进程观察到 TurnStarted。预留区满时
                // 对它施加背压，而不是报错或丢弃终态。
                return Ok(false);
            }
            slots.pending_terminal_turns.insert(terminal_key);
            queue.push_back(queue_thread(commit.clone()));
            self.record_visible_commit();
            return Ok(true);
        }

        let ordinary = queue
            .iter()
            .filter(|entry| entry.is_commit() && entry.terminal_key().is_none())
            .count();
        if ordinary >= NORMAL_PENDING_COMMITS {
            return Ok(false);
        }
        if let Some(started_key) = started_turn_key(commit) {
            let mut slots = self
                .shared
                .settlement_slots
                .lock()
                .map_err(|_| store_error("settlement slot lock poisoned"))?;
            if !slots.active_turns.contains(&started_key)
                && slots.active_turns.len() + slots.pending_terminal_turns.len()
                    >= MAX_PENDING_COMMITS - NORMAL_PENDING_COMMITS
            {
                return Err(store_error(
                    "terminal settlement reserve is exhausted; refusing to start a new lifecycle",
                ));
            }
            slots.active_turns.insert(started_key);
        }
        queue.push_back(queue_thread(commit.clone()));
        self.record_visible_commit();
        Ok(true)
    }

    /// 必须在持有队列锁、且 commit 已经入队后调用。这样 writer 只有在计数
    /// 更新后才能观察并取走该 commit，避免从零计数递减下溢。
    fn record_visible_commit(&self) {
        self.pending_commits.fetch_add(1, Ordering::AcqRel);
    }
}

fn started_turn_key(commit: &ThreadCommit) -> Option<String> {
    commit.facts.runtime_events.iter().find_map(|event| {
        let pl_core::AgentRuntimeEventKind::TurnStarted { turn_id, .. } = &event.kind else {
            return None;
        };
        Some(format!("{}:{turn_id}", commit.agent_id))
    })
}

fn terminal_turn_key(commit: &ThreadCommit) -> Option<String> {
    commit.facts.runtime_events.iter().find_map(|event| {
        let turn_id = match &event.kind {
            pl_core::AgentRuntimeEventKind::TurnFinished { outcome, .. }
            | pl_core::AgentRuntimeEventKind::RecoveryCancelledTurn { outcome, .. } => {
                Some(outcome.turn_id.as_str())
            }
            pl_core::AgentRuntimeEventKind::Faulted { snapshot, .. } => snapshot
                .last_turn
                .as_ref()
                .map(|outcome| outcome.turn_id.as_str())
                .or_else(|| {
                    commit
                        .facts
                        .turn_id
                        .as_ref()
                        .map(|turn_id| turn_id.as_str())
                }),
            pl_core::AgentRuntimeEventKind::Registered { .. }
            | pl_core::AgentRuntimeEventKind::StateChanged { .. }
            | pl_core::AgentRuntimeEventKind::ThreadOpened { .. }
            | pl_core::AgentRuntimeEventKind::TurnQueued { .. }
            | pl_core::AgentRuntimeEventKind::TurnStarted { .. }
            | pl_core::AgentRuntimeEventKind::TurnActivityChanged { .. } => None,
        }?;
        Some(format!("{}:{turn_id}", commit.agent_id))
    })
}

fn try_coalesce_tail(queue: &mut VecDeque<QueueEntry>, next: ThreadCommit) -> bool {
    let Some(QueueEntry::Mutation(QueuedMutation {
        mutation: StudioMutation::Thread(previous),
        ..
    })) = queue.back_mut()
    else {
        return false;
    };
    previous.coalesce(Box::new(next)).is_ok()
}

fn try_coalesce_observed_state(
    queue: &mut VecDeque<QueueEntry>,
    next: &ObservedStateCommit,
) -> bool {
    for entry in queue.iter_mut().rev() {
        match entry {
            QueueEntry::Mutation(QueuedMutation {
                mutation: StudioMutation::Directory(directory),
                ..
            }) if matches!(
                directory.as_ref(),
                StudioDirectoryMutation::ModelPerformance(_)
            ) =>
            {
                let StudioDirectoryMutation::ModelPerformance(previous) = directory.as_mut() else {
                    unreachable!("guard requires model performance mutation")
                };
                if next.revision >= previous.revision {
                    *previous = next.clone();
                }
                return true;
            }
            QueueEntry::Barrier(_) => break,
            QueueEntry::Mutation(_) => {}
        }
    }
    false
}

struct PendingBatch {
    entries: Vec<QueueEntry>,
}

impl PendingBatch {
    fn commit_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.is_commit())
            .count()
    }
}

#[derive(Debug)]
enum BatchError {
    /// 内存是唯一 writer，revision 冲突属于内部错误，不得重试。
    Conflict {
        actual_revision: Option<u64>,
    },
    RetryableStore(PureError),
    BlockedStore(PureError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PersistenceDisposition {
    Retryable,
    Blocked,
}

async fn supervise_writer(shared: Arc<WriterShared>, pending_commits: Arc<AtomicUsize>) {
    loop {
        let worker_shared = shared.clone();
        let worker = tokio::spawn(run_writer(worker_shared, pending_commits.clone()));
        let Err(error) = worker.await else {
            return;
        };
        recover_inflight(&shared);
        publish_blocked(
            &shared,
            &format!("write-behind worker terminated unexpectedly: {error}"),
        );
        fail_barriers(&shared, "write-behind worker terminated unexpectedly");
        if shared.stopping.load(Ordering::Acquire) {
            return;
        }
        shared.retry_notify.notified().await;
        if shared.stopping.load(Ordering::Acquire) {
            return;
        }
    }
}

async fn run_writer(shared: Arc<WriterShared>, pending_commits: Arc<AtomicUsize>) {
    let mut retries = 0usize;
    loop {
        let stopping = shared.stopping.load(Ordering::Acquire);
        let (queued_commits, flush_immediately, oldest_accepted_at) = queued_work(&shared);
        if queued_commits == 0 && !flush_immediately {
            if stopping {
                return;
            }
            shared.work_notify.notified().await;
            continue;
        }
        let deadline = oldest_accepted_at
            .unwrap_or_else(tokio::time::Instant::now)
            .checked_add(FLUSH_INTERVAL)
            .unwrap_or_else(tokio::time::Instant::now);
        if !stopping && queued_commits < MAX_BATCH_COMMITS && !flush_immediately {
            tokio::select! {
                _ = shared.work_notify.notified() => continue,
                _ = tokio::time::sleep_until(deadline) => {}
            }
        }
        let batch = drain_batch(&shared);
        if batch.entries.is_empty() {
            continue;
        }
        let started_at = std::time::Instant::now();
        let commit_count = batch.commit_count();
        let outcome = if commit_count == 0 {
            Ok(())
        } else {
            apply_batch(&shared.store, &batch).await
        };
        match outcome {
            Ok(()) => {
                let was_unhealthy = matches!(
                    shared.state.borrow().state,
                    PersistenceState::Degraded(_) | PersistenceState::Recovering(_)
                );
                retries = 0;
                clear_inflight(&shared);
                advance_batch_durability(&shared, &batch);
                release_applied_settlement_slots(&shared, &batch);
                pending_commits.fetch_sub(commit_count, Ordering::AcqRel);
                complete_applied_batch(batch);
                // 注意：borrow 的 Ref 必须先 drop 再 send_replace，否则读写锁自锁。
                let next_progress = shared.progress.borrow().wrapping_add(1);
                shared.progress.send_replace(next_progress);
                update_after_success(
                    &shared,
                    pending_commits.load(Ordering::Acquire),
                    was_unhealthy,
                );
                tracing::trace!(
                    commits = commit_count,
                    elapsed_ms = started_at.elapsed().as_millis() as u64,
                    "write-behind batch applied"
                );
            }
            Err(BatchError::Conflict { actual_revision }) => {
                requeue_batch(&shared, batch);
                publish_blocked(
                    &shared,
                    &format!(
                        "write-behind revision conflict (actual revision {actual_revision:?}); \
                         memory must be the sole writer"
                    ),
                );
                if shared.stopping.load(Ordering::Acquire) {
                    fail_barriers(&shared, "write-behind writer is blocked");
                    return;
                }
                shared.retry_notify.notified().await;
            }
            Err(BatchError::BlockedStore(error)) => {
                requeue_batch(&shared, batch);
                publish_blocked(&shared, &error.to_string());
                if shared.stopping.load(Ordering::Acquire) {
                    fail_barriers(&shared, "write-behind writer is blocked");
                    return;
                }
                shared.retry_notify.notified().await;
            }
            Err(BatchError::RetryableStore(error)) => {
                retries += 1;
                requeue_batch(&shared, batch);
                if retries <= FAST_BATCH_RETRIES {
                    tracing::warn!(
                        attempt = retries,
                        error_bytes = error.to_string().len(),
                        "write-behind batch failed; retrying"
                    );
                    wait_for_retry(
                        &shared,
                        RETRY_BACKOFF * u32::try_from(retries).unwrap_or(u32::MAX),
                    )
                    .await;
                    continue;
                }
                publish_degraded(&shared, &error);
                let exponent = u32::try_from(retries.saturating_sub(FAST_BATCH_RETRIES + 1))
                    .unwrap_or(u32::MAX)
                    .min(5);
                let backoff = Duration::from_secs(1u64 << exponent).min(MAX_RETRY_BACKOFF);
                wait_for_retry(&shared, backoff).await;
            }
        }
    }
}

fn queued_work(shared: &WriterShared) -> (usize, bool, Option<tokio::time::Instant>) {
    let queue = shared
        .queue
        .lock()
        .expect("write-behind queue lock poisoned");
    (
        queue.iter().filter(|entry| entry.is_commit()).count(),
        queue.iter().any(QueueEntry::flushes_immediately),
        queue.iter().find_map(QueueEntry::accepted_at),
    )
}

async fn wait_for_retry(shared: &WriterShared, backoff: Duration) {
    tokio::select! {
        _ = shared.retry_notify.notified() => {}
        _ = tokio::time::sleep(backoff) => {}
    }
}

/// 从队首取一批 entry：commit 尽量成批，barrier 只在批次末尾收尾。
fn drain_batch(shared: &WriterShared) -> PendingBatch {
    let mut inflight = shared
        .inflight
        .lock()
        .expect("write-behind inflight lock poisoned");
    assert!(
        inflight.is_empty(),
        "write-behind may only own one in-flight batch"
    );
    let mut queue = shared
        .queue
        .lock()
        .expect("write-behind queue lock poisoned");
    let mut entries = Vec::with_capacity(MAX_BATCH_COMMITS);
    while entries.len() < MAX_BATCH_COMMITS {
        match queue.front() {
            Some(QueueEntry::Barrier(_)) if !entries.is_empty() => break,
            Some(entry) => {
                if let Some(commit) = entry.clone_commit() {
                    inflight.push_back(commit);
                }
                entries.push(queue.pop_front().expect("front entry checked"));
            }
            None => break,
        }
    }
    PendingBatch { entries }
}

/// 瞬时失败后把整批按原顺序放回队首等待重试。
fn requeue_batch(shared: &WriterShared, batch: PendingBatch) {
    clear_inflight(shared);
    let mut queue = shared
        .queue
        .lock()
        .expect("write-behind queue lock poisoned");
    for entry in batch.entries.into_iter().rev() {
        queue.push_front(entry);
    }
}

/// 成功应用后丢弃由共享状态持有的 typed snapshot 副本。
fn clear_inflight(shared: &WriterShared) {
    shared
        .inflight
        .lock()
        .expect("write-behind inflight lock poisoned")
        .clear();
}

/// worker panic 后恢复尚未 durable 的 typed mutation。
///
/// barrier 不是业务事实，worker 异常会关闭其 sender；typed mutation 则必须按原序
/// 回到队首，供诊断或显式重试使用。
fn recover_inflight(shared: &WriterShared) {
    let mut inflight = shared
        .inflight
        .lock()
        .expect("write-behind inflight lock poisoned");
    if inflight.is_empty() {
        return;
    }
    let mut queue = shared
        .queue
        .lock()
        .expect("write-behind queue lock poisoned");
    while let Some(entry) = inflight.pop_back() {
        queue.push_front(entry);
    }
}

async fn apply_batch(store: &StudioStore, batch: &PendingBatch) -> Result<(), BatchError> {
    let tx = store.database().begin().await.map_err(classify_db_error)?;
    for entry in &batch.entries {
        match entry {
            QueueEntry::Mutation(QueuedMutation {
                mutation: StudioMutation::Thread(commit),
                ..
            }) => match apply_state_commit(&tx, commit).await {
                Ok(ApplyCommitOutcome::Applied | ApplyCommitOutcome::AlreadyApplied) => {}
                Ok(ApplyCommitOutcome::RevisionConflict { actual_revision }) => {
                    let _ = tx.rollback().await;
                    return Err(BatchError::Conflict { actual_revision });
                }
                Err(error) => {
                    let _ = tx.rollback().await;
                    return Err(classify_store_error(error));
                }
            },
            QueueEntry::Mutation(QueuedMutation {
                mutation: StudioMutation::Directory(directory),
                ..
            }) => match directory.as_ref() {
                StudioDirectoryMutation::Delta(delta) => {
                    if let Err(error) = apply_directory_delta(&tx, delta).await {
                        let _ = tx.rollback().await;
                        return Err(classify_store_error(store_error(error)));
                    }
                }
                StudioDirectoryMutation::ModelPerformance(commit) => {
                    if let Err(error) = put_object(
                        &tx,
                        MODEL_PERFORMANCE_OWNER_ID,
                        &commit.value,
                        commit.value.updated_at(),
                    )
                    .await
                    {
                        let _ = tx.rollback().await;
                        return Err(classify_store_error(store_error(error)));
                    }
                }
            },
            QueueEntry::Barrier(_) => {}
        }
    }
    tx.commit().await.map_err(classify_db_error)?;
    Ok(())
}

fn classify_db_error(error: sea_orm::DbErr) -> BatchError {
    let disposition = db_error_disposition(&error);
    classified_store_error(disposition, store_error(error))
}

fn classify_store_error(error: PureError) -> BatchError {
    let message = error.to_string().to_ascii_lowercase();
    let disposition = if contains_retryable_sqlite_error(&message) {
        PersistenceDisposition::Retryable
    } else {
        PersistenceDisposition::Blocked
    };
    classified_store_error(disposition, error)
}

fn classified_store_error(disposition: PersistenceDisposition, error: PureError) -> BatchError {
    match disposition {
        PersistenceDisposition::Retryable => BatchError::RetryableStore(error),
        PersistenceDisposition::Blocked => BatchError::BlockedStore(error),
    }
}

fn db_error_disposition(error: &sea_orm::DbErr) -> PersistenceDisposition {
    use sea_orm::{ConnAcquireErr, DbErr, RuntimeErr, SqlxError};

    match error {
        DbErr::ConnectionAcquire(ConnAcquireErr::Timeout) => PersistenceDisposition::Retryable,
        DbErr::Conn(RuntimeErr::SqlxError(error))
        | DbErr::Exec(RuntimeErr::SqlxError(error))
        | DbErr::Query(RuntimeErr::SqlxError(error)) => match error.as_ref() {
            SqlxError::Database(error) => error
                .code()
                .as_deref()
                .and_then(|code| code.parse::<i32>().ok())
                .map_or(PersistenceDisposition::Blocked, sqlite_code_disposition),
            SqlxError::Io(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::Interrupted
                        | std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::TimedOut
                ) =>
            {
                PersistenceDisposition::Retryable
            }
            SqlxError::PoolTimedOut => PersistenceDisposition::Retryable,
            _ => PersistenceDisposition::Blocked,
        },
        DbErr::Conn(RuntimeErr::Internal(message))
        | DbErr::Exec(RuntimeErr::Internal(message))
        | DbErr::Query(RuntimeErr::Internal(message))
            if contains_retryable_sqlite_error(&message.to_ascii_lowercase()) =>
        {
            PersistenceDisposition::Retryable
        }
        _ => PersistenceDisposition::Blocked,
    }
}

fn sqlite_code_disposition(extended_code: i32) -> PersistenceDisposition {
    match extended_code & 0xff {
        // SQLITE_BUSY、SQLITE_LOCKED 与 SQLITE_IOERR 允许自动重试。
        5 | 6 | 10 => PersistenceDisposition::Retryable,
        // 损坏、只读、容量耗尽、结构/约束错误等均需要人工处置。
        _ => PersistenceDisposition::Blocked,
    }
}

fn contains_retryable_sqlite_error(message: &str) -> bool {
    message.contains("database is locked")
        || message.contains("database table is locked")
        || message.contains("database is busy")
        || message.contains("disk i/o error")
        || message.contains("sqlite_busy")
        || message.contains("sqlite_locked")
        || message.contains("sqlite_ioerr")
}

fn advance_batch_durability(shared: &WriterShared, batch: &PendingBatch) {
    let mut revisions = HashMap::<String, u64>::new();
    for entry in &batch.entries {
        match entry {
            QueueEntry::Mutation(QueuedMutation {
                mutation: StudioMutation::Thread(commit),
                ..
            }) => {
                revisions
                    .entry(commit.agent_id.to_string())
                    .and_modify(|revision| *revision = (*revision).max(commit.facts.revision))
                    .or_insert(commit.facts.revision);
            }
            QueueEntry::Mutation(QueuedMutation {
                mutation: StudioMutation::Directory(_),
                ..
            })
            | QueueEntry::Barrier(_) => {}
        }
    }
    if revisions.is_empty() {
        return;
    }
    let mut changed = false;
    if !revisions.is_empty() {
        let mut durable = shared
            .durable_revisions
            .lock()
            .expect("durable revision lock poisoned");
        for (owner_id, revision) in revisions {
            let current = durable.entry(owner_id).or_default();
            if revision > *current {
                *current = revision;
                changed = true;
            }
        }
    }
    if changed {
        let next = shared.durable_progress.borrow().wrapping_add(1);
        shared.durable_progress.send_replace(next);
    }
}

fn release_applied_settlement_slots(shared: &WriterShared, batch: &PendingBatch) {
    let applied = batch
        .entries
        .iter()
        .filter_map(QueueEntry::terminal_key)
        .collect::<HashSet<_>>();
    if applied.is_empty() {
        return;
    }
    let mut slots = shared
        .settlement_slots
        .lock()
        .expect("settlement slot lock poisoned");
    for terminal_key in applied {
        slots.pending_terminal_turns.remove(&terminal_key);
    }
}

fn advance_durable_revision(shared: &WriterShared, owner_id: &str, revision: u64) {
    let mut durable = shared
        .durable_revisions
        .lock()
        .expect("durable revision lock poisoned");
    let current = durable.entry(owner_id.to_string()).or_default();
    if revision <= *current {
        return;
    }
    *current = revision;
    drop(durable);
    let next = shared.durable_progress.borrow().wrapping_add(1);
    shared.durable_progress.send_replace(next);
}

/// 成功批次只需要完成显式耐久化屏障；commit 入队时已经返回。
fn complete_applied_batch(batch: PendingBatch) {
    for entry in batch.entries {
        match entry {
            QueueEntry::Mutation(_) => {}
            QueueEntry::Barrier(sender) => {
                let _ = sender.send(Ok(()));
            }
        }
    }
}

fn update_healthy_state(shared: &WriterShared, pending: usize) {
    if matches!(
        shared.state.borrow().state,
        PersistenceState::Degraded(_)
            | PersistenceState::Recovering(_)
            | PersistenceState::Blocked(_)
    ) {
        return;
    }
    let state = if pending == 0 {
        PersistenceState::Ready(ReadyPersistence { pending_commits: 0 })
    } else {
        PersistenceState::Flushing(FlushingPersistence {
            pending_commits: pending as u64,
            oldest_pending_revision: oldest_pending_revision(shared),
        })
    };
    publish_state(shared, state);
}

fn update_after_success(shared: &WriterShared, pending: usize, was_unhealthy: bool) {
    let state = if pending == 0 {
        PersistenceState::Ready(ReadyPersistence { pending_commits: 0 })
    } else if was_unhealthy {
        let first_failed_at = first_failed_at(shared).unwrap_or_else(unix_seconds);
        PersistenceState::Recovering(RecoveringPersistence {
            pending_commits: pending as u64,
            oldest_pending_revision: oldest_pending_revision(shared),
            first_failed_at,
        })
    } else {
        PersistenceState::Flushing(FlushingPersistence {
            pending_commits: pending as u64,
            oldest_pending_revision: oldest_pending_revision(shared),
        })
    };
    publish_state(shared, state);
}

fn publish_degraded(shared: &WriterShared, error: &PureError) {
    let first_failed_at = first_failed_at(shared).unwrap_or_else(unix_seconds);
    let pending = pending_from_queue(shared);
    publish_state(
        shared,
        PersistenceState::Degraded(DegradedPersistence {
            pending_commits: pending as u64,
            oldest_pending_revision: oldest_pending_revision(shared),
            first_failed_at,
            error: persistence_error("persistenceUnavailable", error.to_string(), true),
        }),
    );
}

fn publish_blocked(shared: &WriterShared, reason: &str) {
    tracing::error!(reason, "write-behind writer is blocked");
    let first_failed_at = first_failed_at(shared).unwrap_or_else(unix_seconds);
    let pending = pending_from_queue(shared);
    publish_state(
        shared,
        PersistenceState::Blocked(BlockedPersistence {
            pending_commits: pending as u64,
            oldest_pending_revision: oldest_pending_revision(shared),
            first_failed_at,
            error: persistence_error("persistenceBlocked", reason.to_string(), false),
        }),
    );
}

fn publish_state(shared: &WriterShared, state: PersistenceState) {
    let current = shared.state.borrow().clone();
    if current.state == state {
        return;
    }
    shared.state.send_replace(PersistenceStateSnapshot {
        revision: current.revision.saturating_add(1),
        state,
    });
    let next = shared.durable_progress.borrow().wrapping_add(1);
    shared.durable_progress.send_replace(next);
}

fn persistence_error(code: &str, message: String, retryable: bool) -> StateError {
    StateError {
        code: code.to_string(),
        message,
        retryable,
    }
}

fn first_failed_at(shared: &WriterShared) -> Option<i64> {
    match &shared.state.borrow().state {
        PersistenceState::Degraded(state) => Some(state.first_failed_at),
        PersistenceState::Recovering(state) => Some(state.first_failed_at),
        PersistenceState::Blocked(state) => Some(state.first_failed_at),
        PersistenceState::Ready(_) | PersistenceState::Flushing(_) => None,
    }
}

fn oldest_pending_revision(shared: &WriterShared) -> Option<u64> {
    shared
        .queue
        .lock()
        .expect("write-behind queue lock poisoned")
        .iter()
        .find_map(|entry| match entry {
            QueueEntry::Mutation(QueuedMutation {
                mutation: StudioMutation::Thread(commit),
                ..
            }) => Some(commit.facts.revision),
            QueueEntry::Mutation(QueuedMutation {
                mutation: StudioMutation::Directory(directory),
                ..
            }) => match directory.as_ref() {
                StudioDirectoryMutation::ModelPerformance(commit) => Some(commit.revision),
                StudioDirectoryMutation::Delta(_) => None,
            },
            QueueEntry::Barrier(_) => None,
        })
}

fn pending_from_queue(shared: &WriterShared) -> usize {
    shared
        .queue
        .lock()
        .expect("write-behind queue lock poisoned")
        .iter()
        .filter(|entry| entry.is_commit())
        .count()
}

/// writer 退出时只失败屏障，待落库 commit 保留供诊断。
fn fail_barriers(shared: &WriterShared, reason: &str) {
    let mut queue = shared
        .queue
        .lock()
        .expect("write-behind queue lock poisoned");
    let mut retained = VecDeque::with_capacity(queue.len());
    while let Some(entry) = queue.pop_front() {
        if let QueueEntry::Barrier(sender) = entry {
            let _ = sender.send(Err(store_error(reason.to_string())));
        } else {
            retained.push_back(entry);
        }
    }
    *queue = retained;
}
