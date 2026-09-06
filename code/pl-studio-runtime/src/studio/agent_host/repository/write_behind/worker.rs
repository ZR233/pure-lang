//! write-behind 后台 worker。
//!
//! `run_writer` 是单一状态机：等待工作、成批取出、交给 `apply` 应用，再按错误
//! 类别重试、Degraded 退避或进入 Blocked，成功路径推进耐久修订并唤醒屏障；
//! `supervise_writer` 在 worker panic 后恢复 inflight 并按需重启。

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use crate::PureError;
use crate::studio::PersistenceState;

use super::apply::apply_batch;
use super::durability::{advance_batch_durability, complete_applied_batch};
use super::handle::WriterShared;
use super::queue::{
    FAST_BATCH_RETRIES, FLUSH_INTERVAL, MAX_BATCH_COMMITS, MAX_RETRY_BACKOFF, PendingBatch,
    QueueEntry, RETRY_BACKOFF,
};
use super::state::{fail_barriers, publish_blocked, publish_degraded, update_after_success};

#[derive(Debug)]
pub(super) enum BatchError {
    /// 内存是唯一 writer，revision 冲突属于内部错误，不得重试。
    Conflict {
        actual_revision: Option<u64>,
    },
    RetryableStore(PureError),
    BlockedStore(PureError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PersistenceDisposition {
    Retryable,
    Blocked,
}

pub(super) async fn supervise_writer(shared: Arc<WriterShared>, pending_commits: Arc<AtomicUsize>) {
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
        wait_for_retry(&shared, Duration::from_secs(1)).await;
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
                #[cfg(test)]
                assert!(
                    !shared.panic_after_apply.swap(false, Ordering::AcqRel),
                    "injected worker exit after database commit"
                );
                let was_unhealthy = matches!(
                    shared.state.borrow().state,
                    PersistenceState::Degraded(_) | PersistenceState::Recovering(_)
                );
                retries = 0;
                clear_inflight(&shared);
                advance_batch_durability(&shared, &batch);
                pending_commits.fetch_sub(commit_count, Ordering::AcqRel);
                complete_applied_batch(batch);
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
                fail_barriers(&shared, "write-behind storage is unavailable");
                wait_for_retry(&shared, MAX_RETRY_BACKOFF).await;
            }
            Err(BatchError::RetryableStore(error)) => {
                retries += 1;
                requeue_batch(&shared, batch);
                if shared.stopping.load(Ordering::Acquire) {
                    publish_degraded(&shared, &error);
                    fail_barriers(&shared, "shutdown could not save pending facts");
                    return;
                }
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
