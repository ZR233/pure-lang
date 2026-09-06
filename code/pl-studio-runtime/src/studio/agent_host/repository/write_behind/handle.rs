//! write-behind 队列与后台 writer task 的共享状态及对外句柄。

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use pl_core::ThreadCommit;
use tokio::sync::{Notify, oneshot, watch};
use tokio::task::JoinHandle;

use crate::PureError;
use crate::studio::runtime::ModelPerformanceState;
use crate::studio::store::directory::DirectoryDelta;
use crate::studio::{PersistenceState, PersistenceStateSnapshot, StudioStore};

use super::super::store_error;
use super::durability::advance_durable_revision;
use super::queue::{
    ObservedStateCommit, QueueEntry, queue_directory, queue_model_performance, queue_thread,
    try_coalesce_observed_state,
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
    /// 任一 owner 的耐久修订推进时发布，供精确屏障等待。
    pub(super) durable_progress: watch::Sender<u64>,
    pub(super) durable_revisions: Mutex<HashMap<String, u64>>,
    pub(super) state: watch::Sender<PersistenceStateSnapshot>,
    pub(super) retry_notify: Notify,
    pub(super) stopping: AtomicBool,
    #[cfg(test)]
    pub(super) panic_after_apply: AtomicBool,
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
        let (durable_progress, _) = watch::channel(0u64);
        let (state, _) = watch::channel(PersistenceStateSnapshot::default());
        Self {
            shared: Arc::new(WriterShared {
                store,
                queue: Mutex::new(VecDeque::new()),
                inflight: Mutex::new(VecDeque::new()),
                work_notify: Notify::new(),
                durable_progress,
                durable_revisions: Mutex::new(HashMap::new()),
                state,
                retry_notify: Notify::new(),
                stopping: AtomicBool::new(false),
                #[cfg(test)]
                panic_after_apply: AtomicBool::new(false),
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
        if self.pending_commit_count() > 0 && self.task_is_none() {
            self.shared.stopping.store(false, Ordering::Release);
            self.ensure_task();
        }
        let (sender, receiver) = oneshot::channel();
        drop(receiver);
        self.shared
            .queue
            .lock()
            .expect("write-behind queue lock poisoned")
            .push_back(QueueEntry::Barrier(sender));
        self.shared.retry_notify.notify_one();
        self.shared.work_notify.notify_one();
    }

    pub(in crate::studio) fn block(&self, reason: &str) {
        publish_blocked(&self.shared, reason);
    }

    /// 保留已提交内存事实。数据库状态和积压均不影响此操作。
    pub(in crate::studio) fn record_thread(&self, commit: ThreadCommit) {
        let entry = queue_thread(commit.into());
        self.ensure_task();
        let mut queue = self
            .shared
            .queue
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        queue.push_back(entry);
        self.record_visible_commit();
        drop(queue);
        update_healthy_state(&self.shared, self.pending_commits.load(Ordering::Acquire));
        self.shared.work_notify.notify_one();
    }

    /// 登记已提交目录事实；不检查保存健康或队列容量。
    pub(in crate::studio) fn record_directory(&self, delta: DirectoryDelta) {
        if delta.is_empty() {
            return;
        }
        self.ensure_task();
        let mut queue = self
            .shared
            .queue
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        queue.push_back(queue_directory(delta));
        self.record_visible_commit();
        drop(queue);
        update_healthy_state(&self.shared, self.pending_commit_count());
        self.shared.work_notify.notify_one();
    }

    pub(in crate::studio) fn record_attachments(
        &self,
        records: Vec<crate::studio::AttachmentRecord>,
    ) {
        if records.is_empty() {
            return;
        }
        self.ensure_task();
        let mut queue = self
            .shared
            .queue
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        queue.push_back(super::queue::queue_attachments(records));
        self.record_visible_commit();
        drop(queue);
        update_healthy_state(&self.shared, self.pending_commit_count());
        self.shared.work_notify.notify_one();
    }

    /// 把模型性能 owner 的版本化 typed snapshot 送入同一 write-behind 队列。
    ///
    /// 尚未落库的旧 revision 会被最新完整值覆盖；此处不执行 serde。
    pub(in crate::studio) fn record_model_performance(&self, value: ModelPerformanceState) {
        self.ensure_task();
        let commit = ObservedStateCommit {
            revision: value.revision(),
            value,
        };
        let mut queue = self
            .shared
            .queue
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if try_coalesce_observed_state(&mut queue, &commit) {
            drop(queue);
            self.shared.work_notify.notify_one();
            return;
        }
        queue.push_back(queue_model_performance(commit));
        self.record_visible_commit();
        drop(queue);
        update_healthy_state(&self.shared, self.pending_commits.load(Ordering::Acquire));
        self.shared.work_notify.notify_one();
    }

    pub(in crate::studio) fn is_durable(&self, owner_id: &str, revision: u64) -> bool {
        self.durable_revision(owner_id)
            .is_some_and(|durable| durable >= revision)
            && self
                .has_pending_directory_fact(owner_id)
                .is_ok_and(|pending| !pending)
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

    /// 尝试排空并停止；保存失败返回错误并保留事实。
    pub(in crate::studio) async fn shutdown(&self) -> Result<(), PureError> {
        self.shared.stopping.store(true, Ordering::Release);
        self.shared.work_notify.notify_one();
        self.shared.retry_notify.notify_one();
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
        let mut progress = self.shared.state.subscribe();
        loop {
            self.blocked_result()?;
            if self.pending_commit_count() == 0 {
                return Ok(());
            }
            if self.task_is_none() {
                return Err(store_error(
                    "write-behind writer is stopping with unsaved facts",
                ));
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

    fn task_is_none(&self) -> bool {
        self.task.lock().expect("writer task lock").is_none()
    }

    fn ensure_task(&self) {
        let mut task = self.task.lock().expect("writer task lock");
        if task.as_ref().is_none_or(JoinHandle::is_finished)
            && !self.shared.stopping.load(Ordering::Acquire)
        {
            let shared = self.shared.clone();
            let pending = self.pending_commits.clone();
            *task = Some(tokio::spawn(supervise_writer(shared, pending)));
        }
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

        writer.record_thread(standard_commit(&thread_id, 0));
        writer
            .await_durable(&thread_id, 1)
            .await
            .expect("first revision becomes durable");
        assert_eq!(writer.durable_revision(&thread_id), Some(1));

        writer.record_thread(standard_commit(&thread_id, 1));
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
        writer.record_thread(standard_commit(&thread_id, 2));
        assert_eq!(
            writer.pending_commit_count(),
            1,
            "stopped writer retains unsaved facts"
        );
    }

    #[tokio::test]
    async fn replayed_commit_is_idempotent_via_receipt() {
        let (store, thread_id) = seeded_thread().await;
        let writer = ThreadWriteBehindWriter::new(store.clone());
        writer.seed_durable_revision(&thread_id, 0);

        let commit = standard_commit(&thread_id, 0);
        writer.record_thread(commit.clone());
        writer
            .await_durable(&thread_id, 1)
            .await
            .expect("revision becomes durable");

        // writer 重试或调用方重放同一份 commit：receipt 命中后必须以 AlreadyApplied
        // 吸收，不得报 revision 冲突，也不得重复计数。
        writer.record_thread(commit);
        writer.flush().await.expect("flush barrier completes");
        assert_eq!(writer.pending_commit_count(), 0);
        assert_eq!(writer.durable_revision(&thread_id), Some(1));
        writer.shutdown().await.expect("shutdown writer");
    }
    async fn wait_for_state(
        writer: &ThreadWriteBehindWriter,
        predicate: impl Fn(&crate::PersistenceState) -> bool,
    ) {
        let mut state = writer.subscribe_state();
        tokio::time::timeout(std::time::Duration::from_secs(30), async {
            loop {
                if predicate(&state.borrow_and_update().state) {
                    break;
                }
                state.changed().await.expect("writer state");
            }
        })
        .await
        .expect("writer state must progress");
    }

    async fn fail_database_writes(store: &StudioStore) {
        use sea_orm::ConnectionTrait;
        store.database().execute_unprepared("CREATE TRIGGER fail_writes BEFORE UPDATE ON threads BEGIN SELECT RAISE(ABORT, 'disk i/o error'); END").await.unwrap();
    }

    #[tokio::test]
    async fn database_failure_retains_backlog_beyond_old_capacity_and_catches_up() {
        use sea_orm::ConnectionTrait;
        let (store, id) = seeded_thread().await;
        fail_database_writes(&store).await;
        let writer = ThreadWriteBehindWriter::new(store.clone());
        for revision in 0..1100 {
            writer.record_thread(standard_commit(&id, revision));
        }
        wait_for_state(&writer, |state| {
            matches!(state, crate::PersistenceState::Degraded(_))
        })
        .await;
        assert_eq!(writer.pending_commit_count(), 1100);
        assert!(!writer.is_durable(&id, 1100));
        // 停止尝试报告失败，同时事实仍由共享缓冲持有。
        assert!(writer.shutdown().await.is_err());
        assert_eq!(writer.pending_commit_count(), 1100);
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(1), writer.flush())
                .await
                .expect("stopped flush must finish")
                .is_err()
        );
        store
            .database()
            .execute_unprepared("DROP TRIGGER fail_writes")
            .await
            .unwrap();
        writer.retry_now();
        wait_for_state(&writer, |state| {
            matches!(state, crate::PersistenceState::Ready(_))
        })
        .await;
        assert!(writer.is_durable(&id, 1100));
        assert_eq!(writer.pending_commit_count(), 0);
        let row = crate::studio::entity::thread::Entity::find_by_id(id)
            .one(store.database())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.runtime_revision, Some(1100));
        writer.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn worker_exit_after_transaction_preserves_batch_and_retries_without_duplicates() {
        let (store, id) = seeded_thread().await;
        let writer = ThreadWriteBehindWriter::new(store.clone());
        writer
            .shared
            .panic_after_apply
            .store(true, std::sync::atomic::Ordering::Release);
        let commit_with_message = |revision| {
            let mut commit = standard_commit(&id, revision);
            let session = &mut commit.next_state.session.session;
            for index in 0..=revision {
                session.push_user_prompt(format!("message-{index}"));
            }
            commit.facts.context = Some(pl_core::ThreadContextMutation::Append {
                items: vec![session.snapshot().transcript.last().unwrap().clone()],
            });
            commit
        };
        for revision in 0..4 {
            writer.record_thread(commit_with_message(revision));
        }
        assert!(
            writer.flush().await.is_err(),
            "worker exit must be observable"
        );
        wait_for_state(&writer, |state| {
            matches!(state, crate::PersistenceState::Blocked(_))
        })
        .await;
        assert_eq!(writer.pending_commit_count(), 4);
        for revision in 4..8 {
            writer.record_thread(commit_with_message(revision));
        }
        writer.retry_now();
        wait_for_state(&writer, |state| {
            matches!(state, crate::PersistenceState::Ready(_))
        })
        .await;
        assert!(writer.is_durable(&id, 8));
        let restored = super::super::super::context::restore_transcript(store.database(), &id)
            .await
            .unwrap();
        pretty_assertions::assert_eq!(
            restored,
            commit_with_message(7)
                .next_state
                .session
                .session
                .snapshot()
                .transcript
        );
        let row = crate::studio::entity::thread::Entity::find_by_id(id)
            .one(store.database())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.runtime_revision, Some(8));
        assert_eq!(writer.pending_commit_count(), 0);
        writer.shutdown().await.unwrap();
    }
}
