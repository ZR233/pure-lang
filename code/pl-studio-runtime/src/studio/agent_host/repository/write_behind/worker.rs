//! write-behind 后台 worker。
//!
//! `run_writer` 是单一状态机：等待工作、按条目/字节预算成批取出、交给 `apply` 应用，再按错误
//! 类别重试、Degraded 退避或进入 Blocked；成功路径只在数据库提交后推进 `durable_ticket`。
//! `supervise_writer` 在 worker panic 后恢复 inflight 并按需重启；任何失败都不丢弃待落库事实，
//! 也不会提前确认水位。

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use crate::PureError;
use crate::studio::PersistenceState;

use super::apply::apply_batch;
use super::handle::WriterShared;
use super::queue::{
    FAST_BATCH_RETRIES, FLUSH_INTERVAL, MAX_BATCH_BYTES, MAX_BATCH_COMMITS, MAX_QUEUE_BYTES,
    MAX_QUEUE_ENTRIES, MAX_RETRY_BACKOFF, PendingBatch, RETRY_BACKOFF, queue_pressure,
};
use super::state::{publish_blocked, publish_degraded, update_after_success};

#[derive(Debug)]
pub(super) enum BatchError {
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
        let pressure = queue_pressure(
            &shared
                .queue
                .lock()
                .expect("write-behind queue lock poisoned"),
        );
        if pressure.entries == 0 {
            if stopping {
                return;
            }
            shared.work_notify.notified().await;
            continue;
        }
        if pressure.entries >= MAX_QUEUE_ENTRIES || pressure.bytes >= MAX_QUEUE_BYTES {
            tracing::warn!(
                entries = pressure.entries,
                bytes = pressure.bytes,
                oldest_age_ms = pressure
                    .oldest_accepted_at
                    .map_or(0, |oldest| oldest.elapsed().as_millis() as u64),
                "write-behind queue is under pressure"
            );
        }
        let deadline = pressure
            .oldest_accepted_at
            .unwrap_or_else(tokio::time::Instant::now)
            .checked_add(FLUSH_INTERVAL)
            .unwrap_or_else(tokio::time::Instant::now);
        // 有人显式等待固定 ticket 时不再沿用空闲批量去抖：等待方的屏障水位必须尽快落库。
        // 没人等待（或等待已撤销）时后台写入仍按 FLUSH_INTERVAL 合并。
        let flush_waiting = shared.flush_waiters.load(Ordering::Acquire) > 0;
        let idle_batch = pressure.entries < MAX_BATCH_COMMITS && pressure.bytes < MAX_BATCH_BYTES;
        if !stopping && !flush_waiting && idle_batch {
            tokio::select! {
                _ = shared.work_notify.notified() => continue,
                _ = tokio::time::sleep_until(deadline) => {}
            }
        } else if flush_waiting && idle_batch {
            // 等待方在屏障上：跳过空闲去抖，立即取批落库。
        }
        let batch = drain_batch(&shared);
        if batch.entries.is_empty() {
            continue;
        }
        let started_at = std::time::Instant::now();
        let commit_count = batch.commit_count();
        match apply_batch(&shared.store, &batch).await {
            Ok(()) => {
                let was_unhealthy = matches!(
                    shared.state.borrow().state,
                    PersistenceState::Degraded(_) | PersistenceState::Recovering(_)
                );
                retries = 0;
                clear_inflight(&shared);
                pending_commits.fetch_sub(commit_count, Ordering::AcqRel);
                advance_durable(&shared, &batch);
                update_after_success(
                    &shared,
                    pending_commits.load(Ordering::Acquire),
                    was_unhealthy,
                );
                tracing::trace!(
                    commits = commit_count,
                    bytes = batch.bytes(),
                    elapsed_ms = started_at.elapsed().as_millis() as u64,
                    "write-behind batch applied"
                );
            }
            Err(BatchError::BlockedStore(error)) => {
                requeue_batch(&shared, batch);
                publish_blocked(&shared, &error.to_string());
                if shared.stopping.load(Ordering::Acquire) {
                    return;
                }
                wait_for_retry(&shared, MAX_RETRY_BACKOFF).await;
            }
            Err(BatchError::RetryableStore(error)) => {
                retries += 1;
                requeue_batch(&shared, batch);
                if shared.stopping.load(Ordering::Acquire) {
                    publish_degraded(&shared, &error);
                    return;
                }
                if retries <= FAST_BATCH_RETRIES {
                    tracing::warn!(
                        attempt = retries,
                        error_bytes = error.to_string().len(),
                        "write-behind batch failed; retrying"
                    );
                    let backoff = RETRY_BACKOFF * u32::try_from(retries).unwrap_or(u32::MAX);
                    wait_for_retry(&shared, backoff).await;
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

/// 从队首按条目/字节预算取一批 entry；保持 FIFO 顺序。
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
    let mut entries = Vec::with_capacity(MAX_BATCH_COMMITS.min(queue.len().max(1)));
    let mut bytes = 0usize;
    while entries.len() < MAX_BATCH_COMMITS {
        let Some(front) = queue.front() else {
            break;
        };
        let next_bytes = bytes.saturating_add(front.bytes());
        if !entries.is_empty() && next_bytes > MAX_BATCH_BYTES {
            break;
        }
        bytes = next_bytes;
        let entry = queue.pop_front().expect("front entry checked");
        inflight.push_back(entry.clone());
        entries.push(entry);
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

/// 成功提交后推进耐久 ticket 水位；失败路径不调用本函数。
fn advance_durable(shared: &WriterShared, batch: &PendingBatch) {
    let Some(ticket) = batch.max_ticket() else {
        return;
    };
    let current = *shared.durable_ticket.borrow();
    if ticket > current {
        shared.durable_ticket.send_replace(ticket);
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
/// typed mutation 必须保持原序并回到队首；待落库事实既不确认也不丢弃。
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
