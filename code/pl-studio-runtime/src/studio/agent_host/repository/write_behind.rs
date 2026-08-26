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
use crate::studio::store::directory::{DirectoryDelta, apply_directory_delta};
use crate::studio::task_persistence::{
    ApplyTaskCommitOutcome, TaskPersistenceCommit, apply_task_commit, validate_task_commit,
};
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
/// 空闲时的批落库时间窗口。
const FLUSH_INTERVAL: Duration = Duration::from_millis(500);
/// 进入公开 Degraded 状态前的快速重试次数。
const FAST_BATCH_RETRIES: usize = 3;
/// 瞬时失败的重试退避基值。
const RETRY_BACKOFF: Duration = Duration::from_millis(100);
/// Degraded 后的最大自动重试间隔。
const MAX_RETRY_BACKOFF: Duration = Duration::from_secs(30);

enum QueueEntry {
    ThreadCommit(Box<ThreadCommit>),
    TaskCommit(Box<TaskPersistenceCommit>),
    Directory(Box<DirectoryDelta>),
    Barrier(oneshot::Sender<Result<(), PureError>>),
}

impl QueueEntry {
    const fn is_commit(&self) -> bool {
        matches!(
            self,
            QueueEntry::ThreadCommit(_) | QueueEntry::TaskCommit(_) | QueueEntry::Directory(_)
        )
    }

    fn terminal_key(&self) -> Option<String> {
        match self {
            Self::ThreadCommit(commit) => terminal_turn_key(commit),
            Self::TaskCommit(commit) if commit.ends_lifecycle() => Some(commit.lifecycle_key()),
            Self::TaskCommit(_) | Self::Directory(_) | Self::Barrier(_) => None,
        }
    }
}

struct WriterShared {
    store: StudioStore,
    queue: Mutex<VecDeque<QueueEntry>>,
    /// 入队方唤醒 writer。
    work_notify: Notify,
    /// writer 每次成功排空后发布进度，背压入队方据此重试。
    progress: watch::Sender<u64>,
    /// 任一 owner 的耐久修订推进时发布，供精确屏障等待。
    durable_progress: watch::Sender<u64>,
    durable_revisions: Mutex<HashMap<String, u64>>,
    task_durable_revisions: Mutex<HashMap<String, u64>>,
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
                work_notify: Notify::new(),
                progress,
                durable_progress,
                durable_revisions: Mutex::new(HashMap::new()),
                task_durable_revisions: Mutex::new(HashMap::new()),
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

    /// 返回指定 Task owner 在本进程已确认写入 SQLite 的最高 owner 修订号。
    pub(in crate::studio) fn task_durable_revision(&self, owner_id: &str) -> Option<u64> {
        self.shared
            .task_durable_revisions
            .lock()
            .expect("Task durable revision lock poisoned")
            .get(owner_id)
            .copied()
    }

    /// 记录从 SQLite 恢复出的耐久基线；只允许单调推进。
    pub(in crate::studio) fn seed_durable_revision(&self, owner_id: &str, revision: u64) {
        advance_durable_revision(&self.shared, owner_id, revision);
    }

    /// 记录从 SQLite 恢复出的 Task owner 耐久基线。
    pub(in crate::studio) fn seed_task_durable_revision(&self, owner_id: &str, revision: u64) {
        advance_task_durable_revision(&self.shared, owner_id, revision);
    }

    /// 恢复活动 Task 时登记其终态预留许可。
    pub(in crate::studio) fn seed_task_lifecycle(&self, owner_id: &str, task_run_id: &str) {
        let key = format!("task:{owner_id}:{task_run_id}");
        self.shared
            .settlement_slots
            .lock()
            .expect("settlement slot lock poisoned")
            .active_turns
            .insert(key);
    }

    pub(in crate::studio) fn subscribe_state(&self) -> watch::Receiver<PersistenceStateSnapshot> {
        self.shared.state.subscribe()
    }

    /// 跳过当前退避等待并立即重试队首批次。
    pub(in crate::studio) fn retry_now(&self) {
        self.shared.retry_notify.notify_waiters();
        self.shared.work_notify.notify_one();
    }

    #[cfg(test)]
    fn accepts_new_work(&self) -> bool {
        self.state_snapshot().state.accepts_new_work()
    }

    /// 把一次 commit 送入 write-behind 队列。
    ///
    /// 所有分类都只等待进入进程内队列，不等待 SQLite。
    pub(in crate::studio) async fn enqueue(&self, commit: ThreadCommit) -> Result<(), PureError> {
        self.check_accepting()?;
        self.ensure_task();
        if commit.persistence == PersistenceClass::Coalescible {
            let mut queue = self.lock_queue()?;
            if try_coalesce_tail(&mut queue, commit.clone()) {
                drop(queue);
                self.shared.work_notify.notify_one();
                return Ok(());
            }
        }
        self.enqueue_with_capacity(commit).await?;
        update_healthy_state(&self.shared, self.pending_commits.load(Ordering::Acquire));
        self.shared.work_notify.notify_one();
        Ok(())
    }

    /// 把 TaskRuntime 已提交的完整事实快照送入同一 write-behind 队列。
    pub(in crate::studio) async fn enqueue_task(
        &self,
        commit: TaskPersistenceCommit,
    ) -> Result<(), PureError> {
        validate_task_commit(&commit).map_err(store_error)?;
        self.check_accepting()?;
        self.ensure_task();
        self.enqueue_task_with_capacity(commit).await?;
        update_healthy_state(&self.shared, self.pending_commits.load(Ordering::Acquire));
        self.shared.work_notify.notify_one();
        Ok(())
    }

    /// 把内存目录 owner 已接受的目录事实送入同一 write-behind 队列。
    ///
    /// FIFO 保证注册 delta 先于同 Thread 的首个 state commit 落库；SQLite 失败
    /// 只影响持久化健康状态，不回滚内存目录事实。
    pub(in crate::studio) async fn enqueue_directory(
        &self,
        delta: DirectoryDelta,
    ) -> Result<(), PureError> {
        if delta.is_empty() {
            return Err(store_error("directory delta must carry at least one fact"));
        }
        self.check_accepting()?;
        self.ensure_task();
        let mut progress = self.shared.progress.subscribe();
        let mut pending = Some(delta);
        loop {
            self.check_accepting()?;
            if self.try_enqueue_directory_now(pending.as_ref().expect("delta present"))? {
                drop(pending.take());
                self.record_visible_commit();
                update_healthy_state(&self.shared, self.pending_commits.load(Ordering::Acquire));
                self.shared.work_notify.notify_one();
                return Ok(());
            }
            progress
                .changed()
                .await
                .map_err(|_| store_error("write-behind progress channel closed"))?;
        }
    }

    /// 目录 delta 占用普通容量；队满时对调用方施加背压。
    fn try_enqueue_directory_now(&self, delta: &DirectoryDelta) -> Result<bool, PureError> {
        let mut queue = self.lock_queue()?;
        let ordinary = queue
            .iter()
            .filter(|entry| entry.is_commit() && entry.terminal_key().is_none())
            .count();
        if ordinary >= NORMAL_PENDING_COMMITS {
            return Ok(false);
        }
        queue.push_back(QueueEntry::Directory(Box::new(delta.clone())));
        Ok(true)
    }

    /// 等待一个 owner 的指定修订号被 SQLite 确认。
    pub(in crate::studio) async fn await_durable(
        &self,
        owner_id: &str,
        revision: u64,
    ) -> Result<(), PureError> {
        let mut progress = self.shared.durable_progress.subscribe();
        loop {
            if self
                .durable_revision(owner_id)
                .is_some_and(|durable| durable >= revision)
            {
                return Ok(());
            }
            self.blocked_result()?;
            if self.shared.stopping.load(Ordering::Acquire) && self.task_is_none() {
                return Err(store_error(format!(
                    "write-behind writer stopped before owner {owner_id} revision {revision} became durable"
                )));
            }
            self.ensure_task();
            self.shared.work_notify.notify_one();
            progress
                .changed()
                .await
                .map_err(|_| store_error("write-behind durable progress channel closed"))?;
        }
    }

    /// 等待 Task owner 的指定热修订号被 SQLite 确认。
    pub(in crate::studio) async fn await_task_durable(
        &self,
        owner_id: &str,
        revision: u64,
    ) -> Result<(), PureError> {
        let mut progress = self.shared.durable_progress.subscribe();
        loop {
            if self
                .task_durable_revision(owner_id)
                .is_some_and(|durable| durable >= revision)
            {
                return Ok(());
            }
            self.blocked_result()?;
            if self.shared.stopping.load(Ordering::Acquire) && self.task_is_none() {
                return Err(store_error(format!(
                    "write-behind writer stopped before Task owner {owner_id} revision {revision} became durable"
                )));
            }
            self.ensure_task();
            self.shared.work_notify.notify_one();
            progress
                .changed()
                .await
                .map_err(|_| store_error("write-behind durable progress channel closed"))?;
        }
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
            let _ = task.await;
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
            *task = Some(tokio::spawn(run_writer(shared, pending)));
        }
    }

    /// 普通事实只占用 768 个普通位置；终态提交必须消费已取得的终态许可，
    /// 因而不会被普通积压挤出 256 个预留位置。
    async fn enqueue_with_capacity(&self, commit: ThreadCommit) -> Result<(), PureError> {
        let mut progress = self.shared.progress.subscribe();
        loop {
            self.check_accepting()?;
            if self.try_enqueue_now(&commit)? {
                return Ok(());
            }
            progress
                .changed()
                .await
                .map_err(|_| store_error("write-behind progress channel closed"))?;
        }
    }

    async fn enqueue_task_with_capacity(
        &self,
        commit: TaskPersistenceCommit,
    ) -> Result<(), PureError> {
        let mut progress = self.shared.progress.subscribe();
        loop {
            self.check_accepting()?;
            if self.try_enqueue_task_now(&commit)? {
                return Ok(());
            }
            progress
                .changed()
                .await
                .map_err(|_| store_error("write-behind progress channel closed"))?;
        }
    }

    fn try_enqueue_now(&self, commit: &ThreadCommit) -> Result<bool, PureError> {
        let mut queue = self.lock_queue()?;
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
            queue.push_back(QueueEntry::ThreadCommit(Box::new(commit.clone())));
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
        queue.push_back(QueueEntry::ThreadCommit(Box::new(commit.clone())));
        self.record_visible_commit();
        Ok(true)
    }

    fn try_enqueue_task_now(&self, commit: &TaskPersistenceCommit) -> Result<bool, PureError> {
        let mut queue = self.lock_queue()?;
        let lifecycle_key = commit.lifecycle_key();
        if commit.ends_lifecycle() {
            let mut slots = self
                .shared
                .settlement_slots
                .lock()
                .map_err(|_| store_error("settlement slot lock poisoned"))?;
            if slots.pending_terminal_turns.contains(&lifecycle_key) {
                return Ok(false);
            }
            let consumed_active = slots.active_turns.remove(&lifecycle_key);
            if !consumed_active
                && slots.active_turns.len() + slots.pending_terminal_turns.len()
                    >= MAX_PENDING_COMMITS - NORMAL_PENDING_COMMITS
            {
                return Ok(false);
            }
            slots.pending_terminal_turns.insert(lifecycle_key);
            queue.push_back(QueueEntry::TaskCommit(Box::new(commit.clone())));
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
        if commit.starts_lifecycle() {
            let mut slots = self
                .shared
                .settlement_slots
                .lock()
                .map_err(|_| store_error("settlement slot lock poisoned"))?;
            if !slots.active_turns.contains(&lifecycle_key)
                && slots.active_turns.len() + slots.pending_terminal_turns.len()
                    >= MAX_PENDING_COMMITS - NORMAL_PENDING_COMMITS
            {
                return Err(store_error(
                    "terminal settlement reserve is exhausted; refusing to start a new Task lifecycle",
                ));
            }
            slots.active_turns.insert(lifecycle_key);
        }
        queue.push_back(QueueEntry::TaskCommit(Box::new(commit.clone())));
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
    let Some(QueueEntry::ThreadCommit(previous)) = queue.back_mut() else {
        return false;
    };
    previous.coalesce(Box::new(next)).is_ok()
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

async fn run_writer(shared: Arc<WriterShared>, pending_commits: Arc<AtomicUsize>) {
    let mut interval = tokio::time::interval(FLUSH_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut retries = 0usize;
    loop {
        let stopping = shared.stopping.load(Ordering::Acquire);
        let batch = drain_batch(&shared);
        if batch.entries.is_empty() {
            if stopping {
                return;
            }
            tokio::select! {
                _ = shared.work_notify.notified() => {}
                _ = interval.tick() => {}
            }
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

async fn wait_for_retry(shared: &WriterShared, backoff: Duration) {
    tokio::select! {
        _ = shared.retry_notify.notified() => {}
        _ = tokio::time::sleep(backoff) => {}
    }
}

/// 从队首取一批 entry：commit 尽量成批，barrier 只在批次末尾收尾。
fn drain_batch(shared: &WriterShared) -> PendingBatch {
    let mut queue = shared
        .queue
        .lock()
        .expect("write-behind queue lock poisoned");
    let mut entries = Vec::with_capacity(MAX_BATCH_COMMITS);
    while entries.len() < MAX_BATCH_COMMITS {
        match queue.front() {
            Some(QueueEntry::Barrier(_)) if !entries.is_empty() => break,
            Some(_) => {
                entries.push(queue.pop_front().expect("front entry checked"));
            }
            None => break,
        }
    }
    PendingBatch { entries }
}

/// 瞬时失败后把整批按原顺序放回队首等待重试。
fn requeue_batch(shared: &WriterShared, batch: PendingBatch) {
    let mut queue = shared
        .queue
        .lock()
        .expect("write-behind queue lock poisoned");
    for entry in batch.entries.into_iter().rev() {
        queue.push_front(entry);
    }
}

async fn apply_batch(store: &StudioStore, batch: &PendingBatch) -> Result<(), BatchError> {
    let tx = store.database().begin().await.map_err(classify_db_error)?;
    for entry in &batch.entries {
        match entry {
            QueueEntry::ThreadCommit(commit) => match apply_state_commit(&tx, commit).await {
                Ok(ApplyCommitOutcome::Applied) => {}
                Ok(ApplyCommitOutcome::RevisionConflict { actual_revision }) => {
                    let _ = tx.rollback().await;
                    return Err(BatchError::Conflict { actual_revision });
                }
                Err(error) => {
                    let _ = tx.rollback().await;
                    return Err(classify_store_error(error));
                }
            },
            QueueEntry::TaskCommit(commit) => match apply_task_commit(&tx, commit).await {
                Ok(ApplyTaskCommitOutcome::Applied) => {}
                Ok(ApplyTaskCommitOutcome::RevisionConflict { actual_revision }) => {
                    let _ = tx.rollback().await;
                    return Err(BatchError::Conflict { actual_revision });
                }
                Err(error) => {
                    let _ = tx.rollback().await;
                    return Err(classify_store_error(store_error(error)));
                }
            },
            QueueEntry::Directory(delta) => {
                if let Err(error) = apply_directory_delta(&tx, delta).await {
                    let _ = tx.rollback().await;
                    return Err(classify_store_error(store_error(error)));
                }
            }
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
    let mut task_revisions = HashMap::<String, u64>::new();
    for entry in &batch.entries {
        match entry {
            QueueEntry::ThreadCommit(commit) => {
                revisions
                    .entry(commit.agent_id.to_string())
                    .and_modify(|revision| *revision = (*revision).max(commit.facts.revision))
                    .or_insert(commit.facts.revision);
            }
            QueueEntry::TaskCommit(commit) => {
                task_revisions
                    .entry(commit.owner_id.clone())
                    .and_modify(|revision| *revision = (*revision).max(commit.revision))
                    .or_insert(commit.revision);
            }
            QueueEntry::Directory(_) | QueueEntry::Barrier(_) => {}
        }
    }
    if revisions.is_empty() && task_revisions.is_empty() {
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
    if !task_revisions.is_empty() {
        let mut durable = shared
            .task_durable_revisions
            .lock()
            .expect("Task durable revision lock poisoned");
        for (owner_id, revision) in task_revisions {
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

fn advance_task_durable_revision(shared: &WriterShared, owner_id: &str, revision: u64) {
    let mut durable = shared
        .task_durable_revisions
        .lock()
        .expect("Task durable revision lock poisoned");
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
            QueueEntry::ThreadCommit(_) | QueueEntry::TaskCommit(_) | QueueEntry::Directory(_) => {}
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
            QueueEntry::ThreadCommit(commit) => Some(commit.facts.revision),
            QueueEntry::TaskCommit(commit) => Some(commit.revision),
            QueueEntry::Directory(_) | QueueEntry::Barrier(_) => None,
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use pl_core::{
        AgentCommand, AgentRuntimeEvent, AgentRuntimeEventKind, AgentSnapshotTransition,
        AgentTurnOutcome, DurableCommitFacts, DurableMailboxEnvelope, MailboxBudgetAction,
        MailboxDeliveryState, MailboxInputPayload, TurnId,
    };
    use pl_model::TokenUsage;
    use pl_protocol::{
        FailedTurnState, StateError, TurnFailure, TurnFailureCategory, TurnOutcome, TurnState,
    };
    use sea_orm::{EntityTrait, SqliteTransactionMode, TransactionOptions, TransactionTrait};

    use super::*;
    use crate::StudioMode;
    use crate::studio::agent_host::repository::test_support::{seed_thread, writer_test_commit};
    use crate::studio::entity::{thread, turn};
    use crate::studio::task_coordinator::{
        CreateTaskRun, TaskCommand, TaskFailureKind, TaskOutcome,
    };
    use crate::studio::task_projection;

    async fn task_commit(
        store: &StudioStore,
        label: &str,
        expected_run_revision: Option<u64>,
        snapshot_revision: u64,
    ) -> (String, String, TaskPersistenceCommit) {
        let workspace = std::env::temp_dir().join(format!("pure-writer-{label}"));
        let project = store.upsert_project(&workspace).await.expect("project");
        let task_thread = store
            .create_thread(&project.id, label, StudioMode::Task)
            .await
            .expect("Task Thread");
        let run = store
            .create_task_run(CreateTaskRun {
                project_id: project.id,
                root_thread_id: task_thread.id.clone(),
                request: "implement".to_string(),
                workspace_root: workspace.to_string_lossy().to_string(),
            })
            .await
            .expect("TaskRun");
        let mut aggregate = task_projection::load_task_aggregate(store, &task_thread.id)
            .await
            .expect("load Task aggregate")
            .expect("Task aggregate exists");
        aggregate.run.revision = snapshot_revision;
        aggregate.run.updated_at = aggregate.run.updated_at.saturating_add(1);
        aggregate.refresh_projection().expect("refresh projection");
        (
            task_thread.id.clone(),
            run.id.clone(),
            TaskPersistenceCommit {
                owner_id: task_thread.id,
                expected_owner_revision: 0,
                revision: 1,
                expected_run_revision,
                aggregate,
                stop_events: Vec::new(),
            },
        )
    }

    #[tokio::test]
    async fn standard_enqueue_returns_before_flush_and_flush_waits_for_apply() {
        let store = StudioStore::open_memory().await.expect("memory store");
        let thread_id = seed_thread(&store, "batched").await;
        let writer = ThreadWriteBehindWriter::new(store.clone());

        writer
            .enqueue(writer_test_commit(&thread_id, PersistenceClass::Standard))
            .await
            .expect("standard enqueue");

        writer.flush().await.expect("flush");
        assert_eq!(writer.pending_commit_count(), 0);
        assert!(matches!(
            writer.state_snapshot().state,
            PersistenceState::Ready(_)
        ));
        writer.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn mixed_thread_and_task_batch_rolls_back_atomically_on_task_conflict() {
        let store = StudioStore::open_memory().await.expect("memory store");
        let thread_id = seed_thread(&store, "mixed-rollback-thread").await;
        let (_, task_run_id, conflicting_task) =
            task_commit(&store, "mixed-rollback-task", Some(9), 1).await;
        let batch = PendingBatch {
            entries: vec![
                QueueEntry::ThreadCommit(Box::new(writer_test_commit(
                    &thread_id,
                    PersistenceClass::Standard,
                ))),
                QueueEntry::TaskCommit(Box::new(conflicting_task)),
            ],
        };

        assert!(matches!(
            apply_batch(&store, &batch).await,
            Err(BatchError::Conflict {
                actual_revision: Some(0)
            })
        ));
        assert_eq!(
            thread::Entity::find_by_id(&thread_id)
                .one(store.database())
                .await
                .expect("read Thread")
                .expect("Thread exists")
                .runtime_revision,
            None
        );
        assert_eq!(
            store
                .read_task_run(&task_run_id)
                .await
                .expect("read TaskRun")
                .expect("TaskRun exists")
                .revision,
            0
        );
    }

    #[tokio::test]
    async fn global_flush_advances_thread_and_task_durability_together() {
        let store = StudioStore::open_memory().await.expect("memory store");
        let thread_id = seed_thread(&store, "mixed-durable-thread").await;
        let (task_owner, task_run_id, task) =
            task_commit(&store, "mixed-durable-task", Some(0), 1).await;
        let writer = ThreadWriteBehindWriter::new(store.clone());
        writer
            .enqueue(writer_test_commit(&thread_id, PersistenceClass::Standard))
            .await
            .expect("Thread enqueue");
        writer.enqueue_task(task).await.expect("Task enqueue");

        writer.flush().await.expect("global flush");

        assert_eq!(writer.durable_revision(&thread_id), Some(1));
        assert_eq!(writer.task_durable_revision(&task_owner), Some(1));
        assert_eq!(
            store
                .read_task_run(&task_run_id)
                .await
                .expect("read TaskRun")
                .expect("TaskRun exists")
                .revision,
            1
        );
        writer.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn successful_retry_transitions_degraded_through_recovering_to_ready() {
        let store = StudioStore::open_memory().await.expect("memory store");
        let writer = ThreadWriteBehindWriter::new(store);
        publish_degraded(
            &writer.shared,
            &store_error("database is locked while applying batch"),
        );
        assert!(matches!(
            writer.state_snapshot().state,
            PersistenceState::Degraded(_)
        ));

        update_after_success(&writer.shared, 1, true);
        assert!(matches!(
            writer.state_snapshot().state,
            PersistenceState::Recovering(_)
        ));
        update_after_success(&writer.shared, 0, true);
        assert!(matches!(
            writer.state_snapshot().state,
            PersistenceState::Ready(_)
        ));
    }

    #[tokio::test]
    async fn actual_sqlite_busy_degrades_then_recovers_without_losing_the_commit() {
        let root = tempfile::tempdir().expect("temporary database directory");
        let database_path = root.path().join("studio.sqlite");
        let lock_store = StudioStore::open(&database_path)
            .await
            .expect("lock holder store");
        let store = StudioStore::open(&database_path)
            .await
            .expect("writer store");
        store
            .use_short_busy_timeout_for_test()
            .await
            .expect("short busy timeout");
        let thread_id = seed_thread(&store, "actual-sqlite-busy").await;
        let lock = lock_store
            .database()
            .begin_with_options(TransactionOptions {
                sqlite_transaction_mode: Some(SqliteTransactionMode::Immediate),
                ..Default::default()
            })
            .await
            .expect("hold SQLite write reservation");
        let writer = ThreadWriteBehindWriter::new(store.clone());
        writer
            .enqueue(writer_test_commit(&thread_id, PersistenceClass::Standard))
            .await
            .expect("hot commit accepted");

        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if matches!(writer.state_snapshot().state, PersistenceState::Degraded(_)) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("real SQLite BUSY must enter Degraded after fast retries");
        assert_eq!(writer.pending_commit_count(), 1);
        assert!(!writer.accepts_new_work());

        lock.rollback().await.expect("release SQLite lock");
        writer.retry_now();
        tokio::time::timeout(Duration::from_secs(3), writer.flush())
            .await
            .expect("recovered flush timed out")
            .expect("recovered flush");

        assert_eq!(writer.pending_commit_count(), 0);
        assert_eq!(writer.durable_revision(&thread_id), Some(1));
        assert!(matches!(
            writer.state_snapshot().state,
            PersistenceState::Ready(_)
        ));
        assert!(writer.accepts_new_work());
        writer.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn flush_after_shutdown_starts_waits_for_the_in_flight_sqlite_commit() {
        let root = tempfile::tempdir().expect("temporary database directory");
        let database_path = root.path().join("studio.sqlite");
        let lock_store = StudioStore::open(&database_path)
            .await
            .expect("lock holder store");
        let store = StudioStore::open(&database_path)
            .await
            .expect("writer store");
        store
            .use_short_busy_timeout_for_test()
            .await
            .expect("short busy timeout");
        let thread_id = seed_thread(&store, "shutdown-sqlite-busy").await;
        let lock = lock_store
            .database()
            .begin_with_options(TransactionOptions {
                sqlite_transaction_mode: Some(SqliteTransactionMode::Immediate),
                ..Default::default()
            })
            .await
            .expect("hold SQLite write reservation");
        let writer = ThreadWriteBehindWriter::new(store);
        writer
            .enqueue(writer_test_commit(&thread_id, PersistenceClass::Standard))
            .await
            .expect("hot commit accepted");
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if matches!(writer.state_snapshot().state, PersistenceState::Degraded(_)) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("writer must degrade while the real lock is held");

        let shutdown_writer = writer.clone();
        let shutdown = tokio::spawn(async move { shutdown_writer.shutdown().await });
        while !writer.shared.stopping.load(Ordering::Acquire) {
            tokio::task::yield_now().await;
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(100), writer.flush())
                .await
                .is_err(),
            "flush must not report success while the shutdown writer still has pending data"
        );

        lock.rollback().await.expect("release SQLite lock");
        writer.retry_now();
        tokio::time::timeout(Duration::from_secs(3), shutdown)
            .await
            .expect("shutdown timed out after SQLite recovered")
            .expect("shutdown task")
            .expect("shutdown result");
        assert_eq!(writer.pending_commit_count(), 0);
        assert_eq!(writer.durable_revision(&thread_id), Some(1));
    }

    #[tokio::test]
    async fn queued_commit_is_counted_before_writer_can_drain_it() {
        let store = StudioStore::open_memory().await.expect("memory store");
        let thread_id = seed_thread(&store, "visible-before-counted").await;
        let writer = ThreadWriteBehindWriter::new(store);
        let commit = writer_test_commit(&thread_id, PersistenceClass::Standard);

        assert!(writer.try_enqueue_now(&commit).expect("enqueue commit"));
        assert_eq!(writer.pending_commit_count(), 1);

        let batch = drain_batch(&writer.shared);
        assert_eq!(batch.commit_count(), 1);
        assert_eq!(writer.pending_commit_count(), 1);
    }

    #[test]
    fn sqlite_busy_and_io_errors_are_retryable_but_corruption_is_blocked() {
        use sea_orm::{DbErr, RuntimeErr};

        assert_eq!(
            db_error_disposition(&DbErr::Exec(RuntimeErr::Internal(
                "database is locked".to_string(),
            ))),
            PersistenceDisposition::Retryable
        );
        assert_eq!(
            db_error_disposition(&DbErr::Exec(RuntimeErr::Internal(
                "disk I/O error".to_string(),
            ))),
            PersistenceDisposition::Retryable
        );
        assert_eq!(
            db_error_disposition(&DbErr::Exec(RuntimeErr::Internal(
                "database disk image is malformed".to_string(),
            ))),
            PersistenceDisposition::Blocked
        );
        assert_eq!(
            sqlite_code_disposition(5),
            PersistenceDisposition::Retryable
        );
        assert_eq!(sqlite_code_disposition(11), PersistenceDisposition::Blocked);
        assert_eq!(sqlite_code_disposition(13), PersistenceDisposition::Blocked);
    }

    #[tokio::test]
    async fn owner_durable_barrier_waits_for_the_requested_revision() {
        let store = StudioStore::open_memory().await.expect("memory store");
        let thread_id = seed_thread(&store, "owner-durable").await;
        let writer = ThreadWriteBehindWriter::new(store);
        writer
            .enqueue(writer_test_commit(&thread_id, PersistenceClass::Standard))
            .await
            .expect("enqueue");

        tokio::time::timeout(Duration::from_secs(2), writer.await_durable(&thread_id, 1))
            .await
            .expect("owner barrier timed out")
            .expect("owner barrier");
        assert_eq!(writer.durable_revision(&thread_id), Some(1));
        writer.shutdown().await.expect("shutdown");
    }

    #[test]
    fn consecutive_streaming_commits_coalesce_without_losing_revision_chain() {
        let first = writer_test_commit("thread-coalesce", PersistenceClass::Coalescible);
        let mut next = writer_test_commit("thread-coalesce", PersistenceClass::Coalescible);
        next.expected_revision = Some(1);
        next.next_state.snapshot.revision = 2;
        next.next_state.snapshot.event_sequence = 2;
        next.facts.revision = 2;
        let mut queue = VecDeque::from([QueueEntry::ThreadCommit(Box::new(first))]);

        assert!(try_coalesce_tail(&mut queue, next));
        assert_eq!(queue.len(), 1);
        let QueueEntry::ThreadCommit(commit) = queue.front().expect("coalesced commit") else {
            panic!("tail must remain a commit");
        };
        assert_eq!(commit.expected_revision, None);
        assert_eq!(commit.facts.revision, 2);
        assert_eq!(commit.next_state.snapshot.revision, 2);
    }

    #[test]
    fn streaming_coalescing_cannot_bypass_turn_started_reservation() {
        let first = writer_test_commit("thread-start-license", PersistenceClass::Coalescible);
        let mut next = writer_test_commit("thread-start-license", PersistenceClass::Coalescible);
        next.expected_revision = Some(1);
        next.next_state.snapshot.revision = 2;
        next.facts.revision = 2;
        let turn_id = TurnId::new("turn-start-license").expect("turn id");
        let input = DurableMailboxEnvelope {
            mail_id: "mail-start-license".to_string(),
            turn_id: turn_id.clone(),
            thread_id: next.agent_id.clone(),
            payload: MailboxInputPayload::user("start"),
            queue_coalescing_key: None,
            budget_action: MailboxBudgetAction::Preserve,
            delivery_state: MailboxDeliveryState::default(),
            queued_at: 1,
        };
        next.facts.runtime_events.push(AgentRuntimeEvent {
            agent_id: next.agent_id.clone(),
            sequence: 2,
            created_at: 1,
            kind: AgentRuntimeEventKind::TurnStarted {
                turn_id,
                thread_id: next.agent_id.clone(),
                input,
                claimed_inputs: Vec::new(),
                snapshot: Box::new(next.next_state.snapshot.clone()),
            },
        });
        let mut queue = VecDeque::from([QueueEntry::ThreadCommit(Box::new(first))]);

        assert!(!try_coalesce_tail(&mut queue, next));
        assert_eq!(queue.len(), 1);
    }

    #[tokio::test]
    async fn duplicate_pending_terminal_waits_without_consuming_another_reserve() {
        let store = StudioStore::open_memory().await.expect("memory store");
        let writer = ThreadWriteBehindWriter::new(store);
        let turn_id = TurnId::new("turn-duplicate-terminal").expect("turn id");
        let failure = TurnFailure::permanent(TurnFailureCategory::Internal, "failed");
        let outcome = AgentTurnOutcome {
            turn_id,
            thread_id: pl_core::ThreadId::new("thread-duplicate-terminal").expect("thread id"),
            outcome: TurnOutcome::failed(failure),
            usage: TokenUsage::default(),
            started_at: Some(1),
            finished_at: 2,
        };
        let mut terminal =
            writer_test_commit("thread-duplicate-terminal", PersistenceClass::Settlement);
        terminal.facts.runtime_events.push(AgentRuntimeEvent {
            agent_id: terminal.agent_id.clone(),
            sequence: 1,
            created_at: 2,
            kind: AgentRuntimeEventKind::TurnFinished {
                outcome,
                snapshot: Box::new(terminal.next_state.snapshot.clone()),
            },
        });

        assert!(writer.try_enqueue_now(&terminal).expect("first terminal"));
        assert!(
            !writer
                .try_enqueue_now(&terminal)
                .expect("duplicate terminal")
        );
        let slots = writer
            .shared
            .settlement_slots
            .lock()
            .expect("settlement slots");
        assert_eq!(slots.pending_terminal_turns.len(), 1);
    }

    #[tokio::test]
    async fn duplicate_pending_task_terminal_uses_only_one_reserved_slot() {
        let store = StudioStore::open_memory().await.expect("memory store");
        let (owner_id, task_run_id, mut terminal) =
            task_commit(&store, "duplicate-task-terminal", Some(0), 1).await;
        let decision = terminal
            .aggregate
            .run
            .decide(TaskCommand::Complete {
                outcome: TaskOutcome::Failed {
                    kind: TaskFailureKind::Fatal,
                    summary: "fatal".to_string(),
                    evidence: "typed fault".to_string(),
                    cause: "agent unavailable".to_string(),
                    completed_at: 2,
                },
            })
            .expect("terminal decision");
        terminal.aggregate.run.state = decision.next_state;
        terminal
            .aggregate
            .refresh_projection()
            .expect("terminal projection");
        let writer = ThreadWriteBehindWriter::new(store);
        writer.seed_task_lifecycle(&owner_id, &task_run_id);

        assert!(
            writer
                .try_enqueue_task_now(&terminal)
                .expect("first Task terminal")
        );
        assert!(
            !writer
                .try_enqueue_task_now(&terminal)
                .expect("duplicate Task terminal")
        );
        let slots = writer
            .shared
            .settlement_slots
            .lock()
            .expect("settlement slots");
        assert_eq!(slots.pending_terminal_turns.len(), 1);
        assert!(!slots.active_turns.contains(&terminal.lifecycle_key()));
        assert_eq!(writer.pending_commit_count(), 1);
    }

    #[tokio::test]
    async fn settlement_enqueue_only_waits_for_in_memory_queue() {
        let store = StudioStore::open_memory().await.expect("memory store");
        let thread_id = seed_thread(&store, "immediate").await;
        let writer = ThreadWriteBehindWriter::new(store.clone());

        writer
            .enqueue(writer_test_commit(&thread_id, PersistenceClass::Settlement))
            .await
            .expect("settlement enqueue");

        writer.flush().await.expect("explicit durable barrier");
        writer.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn faulted_agent_persists_failed_turn_without_degrading_writer() {
        let store = StudioStore::open_memory().await.expect("memory store");
        let thread_id = seed_thread(&store, "faulted-turn").await;
        let writer = ThreadWriteBehindWriter::new(store.clone());
        let turn_id = TurnId::new("turn-faulted").expect("turn id");
        let failure =
            TurnFailure::permanent(TurnFailureCategory::Internal, "runtime persistence failed");
        let expected_failure = failure.clone();
        let mut faulted = writer_test_commit(&thread_id, PersistenceClass::Settlement);
        faulted
            .next_state
            .snapshot
            .transition(AgentCommand::Fault {
                error: StateError {
                    code: "agentRuntimeFault".to_string(),
                    message: failure.message.clone(),
                    retryable: false,
                },
                turn_id: Some(turn_id.clone()),
                classification: pl_core::AgentFaultClassification::RecoverableRuntime,
            })
            .expect("fault transition");
        faulted.next_state.snapshot.last_turn = Some(AgentTurnOutcome {
            turn_id: turn_id.clone(),
            thread_id: faulted.agent_id.clone(),
            outcome: TurnOutcome::failed(failure),
            usage: TokenUsage::default(),
            started_at: Some(1),
            finished_at: 2,
        });
        faulted.next_state.snapshot.updated_at = 2;
        faulted.facts =
            DurableCommitFacts::from_state(&faulted.next_state, Vec::new(), Vec::new(), None, None);
        let expected_agent = faulted.next_state.snapshot.state.clone();

        writer
            .enqueue(faulted)
            .await
            .expect("faulted commit must enter the in-memory queue");
        writer.flush().await.expect("faulted commit must persist");
        assert_eq!(writer.pending_commit_count(), 0);

        let persisted_thread = thread::Entity::find_by_id(thread_id.clone())
            .one(store.database())
            .await
            .expect("read thread")
            .expect("persisted thread");
        let persisted_agent: pl_core::AgentState =
            serde_json::from_str(&persisted_thread.state_json).expect("agent state");
        assert_eq!(persisted_agent, expected_agent);

        let persisted_turn = turn::Entity::find_by_id(turn_id.to_string())
            .one(store.database())
            .await
            .expect("read turn")
            .expect("persisted turn");
        let persisted_turn_state: TurnState =
            serde_json::from_str(&persisted_turn.state_json).expect("turn state");
        assert_eq!(
            persisted_turn_state,
            TurnState::Failed(FailedTurnState::new(Some(1), 2, expected_failure))
        );

        let mut follow_up = writer_test_commit(&thread_id, PersistenceClass::Settlement);
        follow_up.expected_revision = Some(1);
        follow_up.next_state.snapshot.revision = 2;
        follow_up.next_state.snapshot.event_sequence = 2;
        follow_up.next_state.snapshot.updated_at = 3;
        follow_up.facts = DurableCommitFacts::from_state(
            &follow_up.next_state,
            Vec::new(),
            Vec::new(),
            None,
            None,
        );
        writer
            .enqueue(follow_up)
            .await
            .expect("writer must accept commits after a faulted agent");
        writer.flush().await.expect("writer remains healthy");
        writer.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn enqueue_rejects_after_shutdown() {
        let store = StudioStore::open_memory().await.expect("memory store");
        let writer = ThreadWriteBehindWriter::new(store.clone());
        writer.shutdown().await.expect("shutdown");
        assert!(
            writer
                .enqueue(writer_test_commit("thread-1", PersistenceClass::Standard,))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn idle_batch_flushes_within_time_window() {
        let store = StudioStore::open_memory().await.expect("memory store");
        let thread_id = seed_thread(&store, "idle-window").await;
        let writer = ThreadWriteBehindWriter::new(store.clone());
        writer
            .enqueue(writer_test_commit(&thread_id, PersistenceClass::Standard))
            .await
            .expect("batched enqueue");
        assert_eq!(writer.pending_commit_count(), 1);

        let drained = tokio::time::timeout(FLUSH_INTERVAL * 4, async {
            while writer.pending_commit_count() > 0 {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await;
        assert!(
            drained.is_ok(),
            "idle batch must flush within the time window"
        );
        writer.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn revision_conflict_blocks_writer_without_dropping_pending_commits() {
        let store = StudioStore::open_memory().await.expect("memory store");
        let thread_id = seed_thread(&store, "conflict").await;
        let writer = ThreadWriteBehindWriter::new(store.clone());
        // 先落库一次注册（runtime_revision None -> 1）。
        writer
            .enqueue(writer_test_commit(&thread_id, PersistenceClass::Settlement))
            .await
            .expect("first registration");
        writer.flush().await.expect("first registration durable");
        // 相同 expected_revision 的重复提交与 DB 状态冲突 -> 内部错误，降级。
        writer
            .enqueue(writer_test_commit(&thread_id, PersistenceClass::Settlement))
            .await
            .expect("conflicting commit remains accepted in memory");
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if matches!(writer.state_snapshot().state, PersistenceState::Blocked(_)) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("writer must publish blocked state");
        assert_eq!(writer.pending_commit_count(), 1);
        assert!(
            writer.flush().await.is_err(),
            "blocked writer fails barriers"
        );
        assert!(!writer.accepts_new_work());
        assert!(
            writer.shutdown().await.is_err(),
            "blocked shutdown reports risk"
        );
    }
}
