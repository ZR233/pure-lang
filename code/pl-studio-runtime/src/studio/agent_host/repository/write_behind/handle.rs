//! write-behind 队列与后台 writer task 的共享状态及对外句柄。

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use tokio::sync::{Notify, oneshot, watch};
use tokio::task::JoinHandle;

use crate::PureError;
use crate::studio::runtime::ModelPerformanceState;
use crate::studio::store::directory::DirectoryDelta;
use crate::studio::{PersistenceState, PersistenceStateSnapshot, StudioStore};

use super::super::store_error;
use super::queue::{
    ObservedStateCommit, QueueEntry, queue_directory, queue_model_performance,
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

    pub(in crate::studio) fn has_pending_directory(&self, owner_id: &str) -> bool {
        if self
            .shared
            .queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .any(|entry| entry.contains_directory_fact_for(owner_id))
        {
            return true;
        }
        self.shared
            .inflight
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .any(|entry| entry.contains_directory_fact_for(owner_id))
    }

    pub(in crate::studio) fn state_snapshot(&self) -> PersistenceStateSnapshot {
        self.shared.state.borrow().clone()
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

    pub(in crate::studio) fn record_worktree_lease(
        &self,
        lease: crate::studio::agent_host::worktree_lease::WorktreeLease,
    ) {
        self.ensure_task();
        let mut queue = self
            .shared
            .queue
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        queue.push_back(super::queue::queue_worktree_lease(lease));
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
