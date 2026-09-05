//! write-behind 队列与后台 writer task 的共享状态及对外句柄。

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use pl_core::{PersistenceClass, ThreadCommit};
use tokio::sync::{Notify, oneshot, watch};
use tokio::task::JoinHandle;

use crate::PureError;
use crate::studio::runtime::ModelPerformanceState;
use crate::studio::store::directory::DirectoryDelta;
use crate::studio::{PersistenceState, PersistenceStateSnapshot, StudioStore};

use super::super::store_error;
use super::durability::advance_durable_revision;
use super::queue::{
    MAX_PENDING_COMMITS, NORMAL_PENDING_COMMITS, ObservedStateCommit, QueueEntry, queue_directory,
    queue_model_performance, queue_thread, started_turn_key, terminal_turn_key,
    try_coalesce_observed_state, try_coalesce_tail,
};
use super::state::{publish_blocked, update_healthy_state};
use super::worker::supervise_writer;

pub(super) struct WriterShared {
    pub(super) store: StudioStore,
    pub(super) queue: Mutex<VecDeque<QueueEntry>>,
    /// 已从 queue 取出、但尚未 durable 的 typed mutation 副本。
    ///
    /// worker panic 时 supervisor 把它原序放回 queue；热状态从不依赖 worker
    /// 局部变量保存唯一一份待写事实。
    pub(super) inflight: Mutex<VecDeque<QueueEntry>>,
    /// 入队方唤醒 writer。
    pub(super) work_notify: Notify,
    /// writer 每次成功排空后发布进度，背压入队方据此重试。
    pub(super) progress: watch::Sender<u64>,
    /// 任一 owner 的耐久修订推进时发布，供精确屏障等待。
    pub(super) durable_progress: watch::Sender<u64>,
    pub(super) durable_revisions: Mutex<HashMap<String, u64>>,
    pub(super) settlement_slots: Mutex<SettlementSlots>,
    pub(super) state: watch::Sender<PersistenceStateSnapshot>,
    pub(super) retry_notify: Notify,
    pub(super) stopping: AtomicBool,
}

#[derive(Default)]
pub(super) struct SettlementSlots {
    active_turns: HashSet<String>,
    /// 已进入队列、但尚未确认落库的终态生命周期。
    ///
    /// 同一生命周期的重复终态必须等待前一条落库，不能重复消费预留许可。
    pub(super) pending_terminal_turns: HashSet<String>,
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

#[cfg(test)]
mod tests {
    use pl_core::{
        AgentIdentity, AgentRoleId, AgentSnapshot, AgentState, DurableCommitFacts,
        PersistenceClass, ThreadActorState, ThreadCommit, ThreadId, ThreadMutation,
    };
    use sea_orm::{ActiveModelTrait, EntityTrait, IntoActiveModel};

    use super::ThreadWriteBehindWriter;
    use crate::studio::StudioStore;

    fn actor_state(thread_id: &str, revision: u64) -> ThreadActorState {
        ThreadActorState {
            snapshot: AgentSnapshot {
                identity: AgentIdentity {
                    id: ThreadId::new(thread_id).expect("thread id"),
                    parent_id: None,
                    role: AgentRoleId::new("planner").expect("role id"),
                    depth: 0,
                },
                state: AgentState::idle(),
                pending_inputs: 0,
                progress: None,
                last_turn: None,
                revision,
                event_sequence: revision,
                updated_at: revision as i64,
            },
            session: pl_core::ThreadContextState::empty(),
            pending_inputs: Default::default(),
            active_input: None,
        }
    }

    fn standard_commit(thread_id: &str, from: u64) -> ThreadCommit {
        let next = actor_state(thread_id, from + 1);
        ThreadCommit {
            agent_id: ThreadId::new(thread_id).expect("thread id"),
            persistence: PersistenceClass::Standard,
            expected_revision: Some(from),
            facts: DurableCommitFacts::from_state(&next, Vec::new(), Vec::new(), None, None),
            next_state: next,
            mutation: ThreadMutation::SnapshotAndQueue,
        }
    }

    async fn seeded_thread() -> (StudioStore, String) {
        let store = StudioStore::open_memory().await.expect("memory store");
        let workspace = std::env::temp_dir().join("write-behind-queue-test");
        let project = store.upsert_project(&workspace).await.expect("project");
        let thread = store
            .create_thread(&project.id, "queue", crate::ThreadModeId::simple())
            .await
            .expect("thread row");
        let mut active = crate::studio::entity::thread::Entity::find_by_id(thread.id.clone())
            .one(store.database())
            .await
            .expect("read thread")
            .expect("thread row exists")
            .into_active_model();
        active.runtime_revision = sea_orm::Set(Some(0));
        active
            .update(store.database())
            .await
            .expect("seed revision");
        (store, thread.id)
    }

    #[tokio::test]
    async fn accepted_commit_advances_durable_ledger_and_thread_row() {
        let (store, thread_id) = seeded_thread().await;
        let writer = ThreadWriteBehindWriter::new(store.clone());
        writer.seed_durable_revision(&thread_id, 0);

        writer
            .accept_thread_with_backpressure(standard_commit(&thread_id, 0))
            .await
            .expect("accept first commit");
        writer
            .await_durable(&thread_id, 1)
            .await
            .expect("first revision becomes durable");
        assert_eq!(writer.durable_revision(&thread_id), Some(1));

        writer
            .accept_thread_with_backpressure(standard_commit(&thread_id, 1))
            .await
            .expect("accept second commit");
        writer
            .await_durable(&thread_id, 2)
            .await
            .expect("second revision becomes durable");
        assert_eq!(writer.durable_revision(&thread_id), Some(2));
        assert_eq!(writer.pending_commit_count(), 0);
        assert!(matches!(
            writer.state_snapshot().state,
            crate::PersistenceState::Ready(_)
        ));

        let row = crate::studio::entity::thread::Entity::find_by_id(thread_id.clone())
            .one(store.database())
            .await
            .expect("read thread")
            .expect("thread row exists");
        assert_eq!(row.runtime_revision, Some(2));

        writer.shutdown().await.expect("shutdown writer");
        let rejected = writer
            .accept_thread_with_backpressure(standard_commit(&thread_id, 2))
            .await;
        assert!(rejected.is_err(), "stopped writer must refuse new commits");
    }

    #[tokio::test]
    async fn replayed_commit_is_idempotent_via_receipt() {
        let (store, thread_id) = seeded_thread().await;
        let writer = ThreadWriteBehindWriter::new(store.clone());
        writer.seed_durable_revision(&thread_id, 0);

        let commit = standard_commit(&thread_id, 0);
        writer
            .accept_thread_with_backpressure(commit.clone())
            .await
            .expect("accept commit");
        writer
            .await_durable(&thread_id, 1)
            .await
            .expect("revision becomes durable");

        // writer 重试或调用方重放同一份 commit：receipt 命中后必须以 AlreadyApplied
        // 吸收，不得报 revision 冲突，也不得重复计数。
        writer
            .accept_thread_with_backpressure(commit)
            .await
            .expect("duplicate commit is absorbed");
        writer.flush().await.expect("flush barrier completes");
        assert_eq!(writer.pending_commit_count(), 0);
        assert_eq!(writer.durable_revision(&thread_id), Some(1));
        writer.shutdown().await.expect("shutdown writer");
    }
}
