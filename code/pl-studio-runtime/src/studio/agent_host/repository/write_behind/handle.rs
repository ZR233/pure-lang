//! write-behind 队列与后台 writer task 的共享状态及对外句柄。

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use tokio::sync::{Notify, watch};
use tokio::task::JoinHandle;

use crate::PureError;
use crate::studio::store::directory::DirectoryDelta;
use crate::studio::{PersistenceState, PersistenceStateSnapshot, StudioStore};

use super::super::store_error;
use super::queue::{QueuedMutation, queue_directory, queue_worktree_lease, try_coalesce_directory};
use super::state::update_healthy_state;
use super::worker::supervise_writer;

pub(super) struct WriterShared {
    pub(super) store: StudioStore,
    pub(super) queue: Mutex<VecDeque<QueuedMutation>>,
    /// 已从 queue 取出、但尚未 durable 的 typed mutation 副本。
    ///
    /// worker panic 时 supervisor 把它原序放回 queue；热状态从不依赖 worker
    /// 局部变量保存唯一一份待写事实。
    pub(super) inflight: Mutex<VecDeque<QueuedMutation>>,
    /// 入队方唤醒 writer。
    pub(super) work_notify: Notify,
    /// 单调受理 ticket；每次入队 `fetch_add(1)`。
    pub(super) admitted_ticket: AtomicU64,
    /// 已 durable 的最高连续 ticket；只在批次成功提交后前进。
    pub(super) durable_ticket: watch::Sender<u64>,
    /// 任一 owner 的耐久修订推进时发布，供精确屏障等待。
    pub(super) durable_progress: watch::Sender<u64>,
    pub(super) state: watch::Sender<PersistenceStateSnapshot>,
    pub(super) retry_notify: Notify,
    pub(super) stopping: AtomicBool,
    /// 正在等待固定 ticket 的显式 flush 请求数。
    ///
    /// 大于 0 时 writer 跳过空闲批量去抖、立即取批落库；等待方在开始等待前先自增再唤醒
    /// writer，返回或取消时由 guard 自减，因此没有任何人等待时后台写入仍按
    /// `FLUSH_INTERVAL` 合并。
    pub(super) flush_waiters: AtomicUsize,
    #[cfg(test)]
    pub(super) panic_after_apply: AtomicBool,
    /// 因显式 flush 请求而跳过空闲去抖的次数；测试观测点，生产构建不编译。
    #[cfg(test)]
    pub(super) flush_debounce_bypasses: AtomicUsize,
}

/// 一次显式固定 ticket 等待的登记。
///
/// drop 时撤销请求：等待方返回或整个 future 被取消（例如观察 worker 被 abort）后，
/// writer 立刻恢复空闲去抖，不会因为遗留计数而永久按“有人等待”处理后台写入。
struct PendingFlushRequest<'a> {
    shared: &'a WriterShared,
}

impl Drop for PendingFlushRequest<'_> {
    fn drop(&mut self) {
        self.shared.flush_waiters.fetch_sub(1, Ordering::AcqRel);
    }
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
        let (durable_ticket, _) = watch::channel(0u64);
        let (durable_progress, _) = watch::channel(0u64);
        let (state, _) = watch::channel(PersistenceStateSnapshot::default());
        Self {
            shared: Arc::new(WriterShared {
                store,
                queue: Mutex::new(VecDeque::new()),
                inflight: Mutex::new(VecDeque::new()),
                work_notify: Notify::new(),
                admitted_ticket: AtomicU64::new(0),
                durable_ticket,
                durable_progress,
                state,
                retry_notify: Notify::new(),
                stopping: AtomicBool::new(false),
                flush_waiters: AtomicUsize::new(0),
                #[cfg(test)]
                panic_after_apply: AtomicBool::new(false),
                #[cfg(test)]
                flush_debounce_bypasses: AtomicUsize::new(0),
            }),
            task: Arc::new(Mutex::new(None)),
            pending_commits: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// 当前尚未落库的 pending commit 数。
    pub(in crate::studio) fn pending_commit_count(&self) -> usize {
        self.pending_commits.load(Ordering::Acquire)
    }

    /// 当前已受理的最高 ticket；`flush_through` 的目标上界。
    pub(in crate::studio) fn admitted_ticket(&self) -> u64 {
        self.shared.admitted_ticket.load(Ordering::Acquire)
    }

    pub(in crate::studio) fn state_snapshot(&self) -> PersistenceStateSnapshot {
        self.shared.state.borrow().clone()
    }

    pub(in crate::studio) fn subscribe_state(&self) -> watch::Receiver<PersistenceStateSnapshot> {
        self.shared.state.subscribe()
    }

    /// 跳过当前退避等待并立即重试队首批次；不丢弃任何待落库事实。
    pub(in crate::studio) fn retry_now(&self) {
        if self.pending_commit_count() > 0 && self.task_is_none() {
            self.shared.stopping.store(false, Ordering::Release);
        }
        self.ensure_task();
        self.shared.retry_notify.notify_one();
        self.shared.work_notify.notify_one();
    }

    /// 登记已提交目录事实；不检查保存健康或队列容量。
    pub(in crate::studio) fn record_directory(&self, delta: DirectoryDelta) {
        if delta.is_empty() {
            return;
        }
        self.ensure_task();
        let mut queue = self.lock_queue_shared();
        if try_coalesce_directory(&mut queue, &delta) {
            drop(queue);
            self.shared.work_notify.notify_one();
            // Merging reuses the queued entry's ticket, so the flush target does not grow here.
            return;
        }
        let ticket = self.next_ticket();
        queue.push_back(queue_directory(ticket, delta));
        drop(queue);
        self.record_visible_commit();
        update_healthy_state(&self.shared, self.pending_commit_count());
        self.shared.work_notify.notify_one();
    }

    pub(in crate::studio) fn record_worktree_lease(
        &self,
        lease: crate::studio::agent_host::worktree_lease::WorktreeLease,
    ) {
        self.ensure_task();
        let ticket = self.next_ticket();
        self.lock_queue_shared()
            .push_back(queue_worktree_lease(ticket, lease));
        self.record_visible_commit();
        update_healthy_state(&self.shared, self.pending_commit_count());
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

    /// 等待当前已受理的全部 pending commit 完成落库。
    pub(in crate::studio) async fn flush(&self) -> Result<(), PureError> {
        self.flush_through(self.admitted_ticket()).await
    }

    /// 只等待调用时固定的目标 ticket 变为 durable，不等待整个系统空闲。
    pub(in crate::studio) async fn flush_through(&self, target: u64) -> Result<(), PureError> {
        if target == 0 || *self.shared.durable_ticket.borrow() >= target {
            return Ok(());
        }
        self.blocked_result()?;
        if self.shared.stopping.load(Ordering::Acquire) {
            return self.await_stopping_drain(target).await;
        }
        if self.task_is_none() {
            return Err(store_error(format!(
                "write-behind writer is not running for ticket {target}"
            )));
        }
        // 显式固定 ticket 等待：登记请求并唤醒 writer，让它跳过空闲批量去抖立即取批。
        let _request = self.request_flush();
        // Durability alone cannot end this wait: a terminal `Blocked` health transition keeps the
        // durable sequence unchanged, so the waiter has to observe health as well. Health is
        // subscribed before the first check, so a `Blocked` published before that check is reported
        // by `blocked_result()` and one published after it wakes `health.changed()`; either way the
        // fixed ticket wait returns the real diagnostic instead of waiting for progress that a
        // blocked ticket can never make. Transient `Degraded`/`Recovering` states stay non-fatal.
        let mut progress = self.shared.durable_ticket.subscribe();
        let mut health = self.shared.state.subscribe();
        loop {
            if *progress.borrow_and_update() >= target {
                return Ok(());
            }
            self.blocked_result()?;
            if self.shared.stopping.load(Ordering::Acquire) {
                return self.await_stopping_drain(target).await;
            }
            if self.task_is_none() {
                return Err(store_error(format!(
                    "write-behind writer stopped before ticket {target}"
                )));
            }
            tokio::select! {
                changed = progress.changed() => {
                    changed.map_err(|_| store_error("write-behind durability channel closed"))?;
                }
                changed = health.changed() => {
                    changed.map_err(|_| store_error("write-behind health channel closed"))?;
                }
            }
        }
    }

    /// 登记一次显式固定 ticket 等待，并唤醒 writer 处理已受理批次。
    ///
    /// 必须先自增计数再唤醒：writer 每次循环都重新读取该计数，因此“请求 → 立即取批”
    /// 不受读取压力与进入等待之间的竞态影响；`PendingFlushRequest` 在等待结束时撤销。
    fn request_flush(&self) -> PendingFlushRequest<'_> {
        self.shared.flush_waiters.fetch_add(1, Ordering::AcqRel);
        self.shared.work_notify.notify_one();
        PendingFlushRequest {
            shared: &self.shared,
        }
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

    async fn await_stopping_drain(&self, target: u64) -> Result<(), PureError> {
        let mut progress = self.shared.state.subscribe();
        let mut durability = self.shared.durable_ticket.subscribe();
        loop {
            self.blocked_result()?;
            if *durability.borrow_and_update() >= target {
                return Ok(());
            }
            if self.pending_commit_count() == 0 {
                return Err(store_error(format!(
                    "write-behind writer stopped before ticket {target}"
                )));
            }
            if self.task_is_none() {
                return Err(store_error(
                    "write-behind writer is stopping with unsaved facts",
                ));
            }
            tokio::select! {
                changed = progress.changed() => {
                    changed.map_err(|_| store_error("write-behind shutdown progress channel closed"))?;
                }
                changed = durability.changed() => {
                    changed.map_err(|_| store_error("write-behind shutdown durability channel closed"))?;
                }
            }
        }
    }

    fn lock_queue_shared(&self) -> std::sync::MutexGuard<'_, VecDeque<QueuedMutation>> {
        self.shared
            .queue
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    fn task_is_none(&self) -> bool {
        self.task.lock().expect("writer task lock").is_none()
    }

    fn next_ticket(&self) -> u64 {
        self.shared
            .admitted_ticket
            .fetch_add(1, Ordering::AcqRel)
            .wrapping_add(1)
    }

    fn ensure_task(&self) {
        let mut task = self.task.lock().expect("writer task lock");
        if task.as_ref().is_none_or(JoinHandle::is_finished) {
            if self.shared.stopping.load(Ordering::Acquire) {
                return;
            }
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
    use super::super::state::{publish_blocked, publish_degraded};
    use super::*;

    /// A fixed accepted ticket that can no longer become durable must end the wait with the real
    /// terminal diagnostic: `Blocked` never advances the durable sequence, so durability alone
    /// cannot wake the waiter. A transient `Degraded` state must keep the same wait pending.
    #[tokio::test]
    async fn blocked_health_ends_a_fixed_ticket_flush_without_durable_progress() {
        let store = StudioStore::open_memory()
            .await
            .expect("in-memory product store");
        let writer = ThreadWriteBehindWriter::new(store);
        // Start the real writer task, so the wait below is a wait and not the "not running" guard.
        writer.ensure_task();
        publish_degraded(
            &writer.shared,
            &PureError::MemoryError("transient conflict".to_string()),
        );
        let flush = {
            let writer = writer.clone();
            tokio::spawn(async move { writer.flush_through(1).await })
        };
        // Give the spawned waiter the chance to observe the transient state; a `Degraded` state is
        // auto-retried, so the flush must still be pending and must not report the ticket durable.
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
        assert!(
            !flush.is_finished(),
            "a transient Degraded state must not terminate a fixed-ticket flush"
        );
        assert_eq!(*writer.shared.durable_ticket.borrow(), 0);
        publish_blocked(&writer.shared, "permanent conflict 787");
        let error = flush
            .await
            .expect("flush task must not panic")
            .expect_err("a blocked ticket must fail the flush instead of waiting for progress");
        assert!(
            error.to_string().contains("permanent conflict 787"),
            "flush must surface the real blocked diagnostic: {error}"
        );
        // The blocked ticket is reported, never confirmed: nothing admitted it, nothing is durable.
        assert_eq!(writer.admitted_ticket(), 0);
        assert_eq!(writer.pending_commit_count(), 0);
        assert_eq!(*writer.shared.durable_ticket.borrow(), 0);
    }

    /// A `Blocked` state published before the wait starts is terminal for that fixed ticket too.
    #[tokio::test]
    async fn already_blocked_writer_reports_blocked_for_a_fixed_ticket() {
        let store = StudioStore::open_memory()
            .await
            .expect("in-memory product store");
        let writer = ThreadWriteBehindWriter::new(store);
        publish_blocked(&writer.shared, "blocked before the wait 787");
        let error = writer
            .flush_through(1)
            .await
            .expect_err("a blocked writer must not wait for a ticket it cannot make durable");
        assert!(
            error.to_string().contains("blocked before the wait 787"),
            "flush must surface the real blocked diagnostic: {error}"
        );
        assert_eq!(*writer.shared.durable_ticket.borrow(), 0);
    }

    /// An explicitly requested fixed ticket must be served by waking the writer instead of inheriting
    /// the idle `FLUSH_INTERVAL` batch timer, while an unrequested background write keeps that
    /// debounce. Payload durability is not relaxed: the durable ticket advances only through the real
    /// SQLite commit, and the committed canonical catalog summary is readable afterwards.
    #[tokio::test]
    async fn explicit_flush_request_bypasses_the_idle_batch_timer() {
        let store = StudioStore::open_memory()
            .await
            .expect("in-memory product store");
        let writer = ThreadWriteBehindWriter::new(store.clone());
        let workspace = tempfile::tempdir().expect("workspace directory");
        let project = store
            .upsert_project(workspace.path())
            .await
            .expect("project row");
        let (delta, thread) = DirectoryDelta::register_root_thread(
            crate::studio::ids::new_id("thread"),
            &project.id,
            "flush promptness",
            pl_protocol::ThreadModeId::simple(),
            pl_protocol::ThreadWorkspaceMode::Local,
            project.path.clone(),
        );
        writer.record_directory(delta);
        let target = writer.admitted_ticket();
        assert_eq!(target, 1, "the directory fact is admitted as ticket 1");
        // Nobody is waiting yet: the background write must stay inside the idle batch window.
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
        assert_eq!(
            *writer.shared.durable_ticket.borrow(),
            0,
            "an unrequested background write must keep its idle debounce"
        );
        assert_eq!(
            writer
                .shared
                .flush_debounce_bypasses
                .load(Ordering::Acquire),
            0,
            "no explicit flush request was waiting, so no debounce may be bypassed"
        );
        writer
            .flush_through(target)
            .await
            .expect("an explicitly requested fixed ticket must become durable");
        assert!(
            writer
                .shared
                .flush_debounce_bypasses
                .load(Ordering::Acquire)
                > 0,
            "the fixed-ticket request must wake the writer instead of the idle batch timer"
        );
        assert_eq!(*writer.shared.durable_ticket.borrow(), target);
        assert_eq!(writer.pending_commit_count(), 0);
        assert!(
            store
                .read_thread(&thread.id)
                .await
                .expect("read thread")
                .is_some(),
            "the flushed fact must be readable, not merely counted durable"
        );
        writer
            .shutdown()
            .await
            .expect("writer drains after the flush");
    }
}
