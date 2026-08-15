//! Thread commit 的 write-behind 批量落库 writer。
//!
//! 内存 snapshot 是唯一权威实例；commit 先进入本队列，由后台 task 按 FIFO
//! 分批在单个 SQLite 事务中应用。`Immediate` 边界与 barrier flush 会等待
//! 包含自身的事务完成后才返回。任何落库冲突或不可恢复失败都会让 writer 进入
//! degraded 终态：失败批次与其后所有等待方收到错误，剩余积压保持原地并使
//! 新入队立即失败——fail-stop，不静默重试或丢弃。

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pl_core::{CommitDurability, ThreadCommit, ThreadCommitOutcome};
use sea_orm::TransactionTrait;
use tokio::sync::{Notify, oneshot, watch};
use tokio::task::JoinHandle;

use crate::PureError;
use crate::studio::StudioStore;

use super::{apply_state_commit, store_error};

/// 单批最多应用的 commit 数；一批共享一个 SQLite 事务。
const MAX_BATCH_COMMITS: usize = 64;
/// 队列积压上限；达到后入队方等待 writer 追赶（背压而不是丢弃）。
const MAX_PENDING_COMMITS: usize = 1024;
/// 空闲时的批落库时间窗口。
const FLUSH_INTERVAL: Duration = Duration::from_millis(500);
/// 瞬时落库失败的最大重试次数。
const MAX_BATCH_RETRIES: usize = 3;
/// 瞬时失败的重试退避基值。
const RETRY_BACKOFF: Duration = Duration::from_millis(100);

enum QueueEntry {
    Commit {
        commit: Box<ThreadCommit>,
        completion: Option<oneshot::Sender<Result<ThreadCommitOutcome, PureError>>>,
    },
    Barrier(oneshot::Sender<Result<(), PureError>>),
}

impl QueueEntry {
    const fn is_commit(&self) -> bool {
        matches!(self, QueueEntry::Commit { .. })
    }
}

struct WriterShared {
    store: StudioStore,
    queue: Mutex<VecDeque<QueueEntry>>,
    /// 入队方唤醒 writer。
    work_notify: Notify,
    /// writer 每次成功排空后发布进度，背压入队方据此重试。
    progress: watch::Sender<u64>,
    degraded: Mutex<Option<PureError>>,
    stopping: AtomicBool,
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
        Self {
            shared: Arc::new(WriterShared {
                store,
                queue: Mutex::new(VecDeque::new()),
                work_notify: Notify::new(),
                progress,
                degraded: Mutex::new(None),
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

    /// 把一次 commit 送入 write-behind 队列。
    ///
    /// `Batched` 入队后立即返回 `Applied`；`Immediate` 等待包含该 commit 的
    /// 事务完成后才返回，落库冲突映射为 `RevisionConflict`。
    pub(in crate::studio) async fn enqueue(
        &self,
        commit: ThreadCommit,
    ) -> Result<ThreadCommitOutcome, PureError> {
        self.check_accepting()?;
        self.ensure_task();
        match commit.durability {
            CommitDurability::Batched => {
                self.await_capacity().await?;
                {
                    let mut queue = self.lock_queue()?;
                    queue.push_back(QueueEntry::Commit {
                        commit: Box::new(commit),
                        completion: None,
                    });
                }
                self.pending_commits.fetch_add(1, Ordering::AcqRel);
                self.shared.work_notify.notify_one();
                Ok(ThreadCommitOutcome::Applied)
            }
            CommitDurability::Immediate => {
                let (sender, receiver) = oneshot::channel();
                self.await_capacity().await?;
                {
                    let mut queue = self.lock_queue()?;
                    queue.push_back(QueueEntry::Commit {
                        commit: Box::new(commit),
                        completion: Some(sender),
                    });
                }
                self.pending_commits.fetch_add(1, Ordering::AcqRel);
                self.shared.work_notify.notify_one();
                receiver
                    .await
                    .map_err(|_| store_error("write-behind writer dropped an immediate commit"))?
            }
        }
    }

    /// 等待当前队列中全部（含指定 Thread 的）pending commit 完成落库。
    pub(in crate::studio) async fn flush(&self) -> Result<(), PureError> {
        self.degraded_result()?;
        if self.shared.stopping.load(Ordering::Acquire) {
            // writer 只在排空队列后才退出；degraded 已在上面返回错误。
            return Ok(());
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

    /// 排空队列、停止 writer task，并返回累积的 degraded 错误。
    pub(in crate::studio) async fn shutdown(&self) -> Result<(), PureError> {
        self.shared.stopping.store(true, Ordering::Release);
        self.shared.work_notify.notify_one();
        let task = self.task.lock().expect("writer task lock").take();
        if let Some(task) = task {
            let _ = task.await;
        }
        self.degraded_result()
    }

    fn lock_queue(&self) -> Result<std::sync::MutexGuard<'_, VecDeque<QueueEntry>>, PureError> {
        self.shared
            .queue
            .lock()
            .map_err(|_| store_error("write-behind queue lock poisoned"))
    }

    fn check_accepting(&self) -> Result<(), PureError> {
        self.degraded_result()?;
        if self.shared.stopping.load(Ordering::Acquire) {
            return Err(store_error(
                "write-behind writer is shutting down and no longer accepts commits",
            ));
        }
        Ok(())
    }

    fn degraded_result(&self) -> Result<(), PureError> {
        match self
            .shared
            .degraded
            .lock()
            .expect("writer degraded lock")
            .as_ref()
        {
            None => Ok(()),
            Some(error) => Err(store_error(format!(
                "write-behind writer degraded: {error:#}"
            ))),
        }
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

    /// 队列达到积压上限时等待 writer 排空，形成背压而不是丢弃。
    async fn await_capacity(&self) -> Result<(), PureError> {
        let mut progress = self.shared.progress.subscribe();
        loop {
            self.check_accepting()?;
            if self.lock_queue()?.len() < MAX_PENDING_COMMITS {
                return Ok(());
            }
            progress
                .changed()
                .await
                .map_err(|_| store_error("write-behind progress channel closed"))?;
        }
    }
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
    Store(PureError),
}

/// 一批事务的最终结局；决定各等待方收到的结果。
enum BatchSettlement {
    Applied,
    Conflict { actual_revision: Option<u64> },
    Failed,
}

async fn run_writer(shared: Arc<WriterShared>, pending_commits: Arc<AtomicUsize>) {
    let mut interval = tokio::time::interval(FLUSH_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut retries = 0usize;
    loop {
        let stopping = shared.stopping.load(Ordering::Acquire);
        let batch = drain_batch(&shared, &pending_commits);
        if batch.entries.is_empty() {
            if stopping {
                fail_remaining_waiters(&shared);
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
                retries = 0;
                complete_batch(batch, BatchSettlement::Applied);
                // 注意：borrow 的 Ref 必须先 drop 再 send_replace，否则读写锁自锁。
                let next_progress = shared.progress.borrow().wrapping_add(1);
                shared.progress.send_replace(next_progress);
                tracing::trace!(
                    commits = commit_count,
                    elapsed_ms = started_at.elapsed().as_millis() as u64,
                    "write-behind batch applied"
                );
            }
            Err(BatchError::Conflict { actual_revision }) => {
                degrade(
                    &shared,
                    &format!(
                        "write-behind revision conflict (actual revision {actual_revision:?}); \
                         memory must be the sole writer"
                    ),
                );
                complete_batch(batch, BatchSettlement::Conflict { actual_revision });
                fail_remaining_waiters(&shared);
                return;
            }
            Err(BatchError::Store(error)) => {
                retries += 1;
                if retries <= MAX_BATCH_RETRIES {
                    tracing::warn!(
                        attempt = retries,
                        error_bytes = error.to_string().len(),
                        "write-behind batch failed; retrying"
                    );
                    requeue_batch(&shared, batch, &pending_commits);
                    tokio::time::sleep(RETRY_BACKOFF * retries as u32).await;
                    continue;
                }
                degrade(&shared, &format!("write-behind batch failed: {error:#}"));
                complete_batch(batch, BatchSettlement::Failed);
                fail_remaining_waiters(&shared);
                return;
            }
        }
    }
}

/// 从队首取一批 entry：commit 尽量成批，barrier 只在批次末尾收尾。
fn drain_batch(shared: &WriterShared, pending_commits: &AtomicUsize) -> PendingBatch {
    let mut queue = shared
        .queue
        .lock()
        .expect("write-behind queue lock poisoned");
    let mut entries = Vec::with_capacity(MAX_BATCH_COMMITS);
    while entries.len() < MAX_BATCH_COMMITS {
        match queue.front() {
            Some(QueueEntry::Barrier(_)) if !entries.is_empty() => break,
            Some(entry) => {
                if entry.is_commit() {
                    pending_commits.fetch_sub(1, Ordering::AcqRel);
                }
                entries.push(queue.pop_front().expect("front entry checked"));
            }
            None => break,
        }
    }
    PendingBatch { entries }
}

/// 瞬时失败后把整批按原顺序放回队首等待重试。
fn requeue_batch(shared: &WriterShared, batch: PendingBatch, pending_commits: &AtomicUsize) {
    let mut queue = shared
        .queue
        .lock()
        .expect("write-behind queue lock poisoned");
    for entry in batch.entries.into_iter().rev() {
        if entry.is_commit() {
            pending_commits.fetch_add(1, Ordering::AcqRel);
        }
        queue.push_front(entry);
    }
}

async fn apply_batch(store: &StudioStore, batch: &PendingBatch) -> Result<(), BatchError> {
    let tx = store
        .database()
        .begin()
        .await
        .map_err(|error| BatchError::Store(store_error(error.to_string())))?;
    for entry in &batch.entries {
        let QueueEntry::Commit { commit, .. } = entry else {
            continue;
        };
        match apply_state_commit(&tx, commit).await {
            Ok(ThreadCommitOutcome::Applied) => {}
            Ok(ThreadCommitOutcome::RevisionConflict { actual_revision }) => {
                let _ = tx.rollback().await;
                return Err(BatchError::Conflict { actual_revision });
            }
            Err(error) => {
                let _ = tx.rollback().await;
                return Err(BatchError::Store(error));
            }
        }
    }
    tx.commit()
        .await
        .map_err(|error| BatchError::Store(store_error(error.to_string())))?;
    Ok(())
}

/// 完成批次内的等待方；冲突按 `RevisionConflict` 原样返回给调用方。
fn complete_batch(batch: PendingBatch, settlement: BatchSettlement) {
    for entry in batch.entries {
        match entry {
            QueueEntry::Commit { completion, .. } => {
                if let Some(sender) = completion {
                    let _ = sender.send(match &settlement {
                        BatchSettlement::Applied => Ok(ThreadCommitOutcome::Applied),
                        BatchSettlement::Conflict { actual_revision } => {
                            Ok(ThreadCommitOutcome::RevisionConflict {
                                actual_revision: *actual_revision,
                            })
                        }
                        BatchSettlement::Failed => {
                            Err(store_error("write-behind batch transaction failed"))
                        }
                    });
                }
            }
            QueueEntry::Barrier(sender) => {
                let _ = sender.send(match settlement {
                    BatchSettlement::Applied => Ok(()),
                    BatchSettlement::Conflict { .. } | BatchSettlement::Failed => {
                        Err(store_error("write-behind flush failed"))
                    }
                });
            }
        }
    }
}

fn degrade(shared: &WriterShared, reason: &str) {
    tracing::error!(reason, "write-behind writer is degrading");
    let mut degraded = shared.degraded.lock().expect("writer degraded lock");
    if degraded.is_none() {
        *degraded = Some(store_error(reason.to_string()));
    }
}

/// 降级后使队列中尚未处理的等待方全部失败；Batched commit 保留在原地。
fn fail_remaining_waiters(shared: &WriterShared) {
    let mut queue = shared
        .queue
        .lock()
        .expect("write-behind queue lock poisoned");
    for entry in queue.drain(..) {
        match entry {
            QueueEntry::Commit { completion, .. } => {
                if let Some(sender) = completion {
                    let _ = sender.send(Err(store_error(
                        "write-behind writer degraded before this commit was applied",
                    )));
                }
            }
            QueueEntry::Barrier(sender) => {
                let _ = sender.send(Err(store_error(
                    "write-behind writer degraded before this flush completed",
                )));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::studio::agent_host::repository::test_support::{seed_thread, writer_test_commit};

    #[tokio::test]
    async fn batched_enqueue_returns_before_flush_and_flush_waits_for_apply() {
        let store = StudioStore::open_memory().await.expect("memory store");
        let thread_id = seed_thread(&store, "batched").await;
        let writer = ThreadWriteBehindWriter::new(store.clone());

        let outcome = writer
            .enqueue(writer_test_commit(&thread_id, CommitDurability::Batched))
            .await
            .expect("batched enqueue");
        assert!(matches!(outcome, ThreadCommitOutcome::Applied));
        assert_eq!(writer.pending_commit_count(), 1);

        writer.flush().await.expect("flush");
        assert_eq!(writer.pending_commit_count(), 0);
        writer.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn immediate_enqueue_waits_for_durable_apply() {
        let store = StudioStore::open_memory().await.expect("memory store");
        let thread_id = seed_thread(&store, "immediate").await;
        let writer = ThreadWriteBehindWriter::new(store.clone());

        writer
            .enqueue(writer_test_commit(&thread_id, CommitDurability::Immediate))
            .await
            .expect("immediate enqueue resolves after flush");

        assert_eq!(writer.pending_commit_count(), 0);
        writer.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn enqueue_rejects_after_shutdown() {
        let store = StudioStore::open_memory().await.expect("memory store");
        let writer = ThreadWriteBehindWriter::new(store.clone());
        writer.shutdown().await.expect("shutdown");
        assert!(
            writer
                .enqueue(writer_test_commit("thread-1", CommitDurability::Batched))
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
            .enqueue(writer_test_commit(&thread_id, CommitDurability::Batched))
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
    async fn revision_conflict_degrades_writer_and_fails_waiters() {
        let store = StudioStore::open_memory().await.expect("memory store");
        let thread_id = seed_thread(&store, "conflict").await;
        let writer = ThreadWriteBehindWriter::new(store.clone());
        // 先落库一次注册（runtime_revision None -> 1）。
        writer
            .enqueue(writer_test_commit(&thread_id, CommitDurability::Immediate))
            .await
            .expect("first registration");
        // 相同 expected_revision 的重复提交与 DB 状态冲突 -> 内部错误，降级。
        let conflicted = writer
            .enqueue(writer_test_commit(&thread_id, CommitDurability::Immediate))
            .await
            .expect("conflict maps to RevisionConflict outcome");
        assert!(matches!(
            conflicted,
            ThreadCommitOutcome::RevisionConflict { .. }
        ));

        assert!(writer.flush().await.is_err(), "degraded writer fails flush");
        assert!(
            writer
                .enqueue(writer_test_commit("another", CommitDurability::Batched))
                .await
                .is_err(),
            "degraded writer rejects new commits"
        );
    }
}
