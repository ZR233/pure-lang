//! write-behind 队列的批量常量、typed mutation 条目、合并策略与批量分组。
//!
//! 每次受理分配单调 `ticket`；队列按条目数/字节/年龄上报压力，并对可合并的目录事实做有界
//! 合并。`flush_through(ticket)` 只等待调用时固定的目标水位（见 design/15 §15.8）。
//!
//! 模型调用/计费事实不是本队列的事实源：它们经 `storage::calls::CallsStore` 的唯一逻辑 writer
//! 落库，本模块只承载目录事实与 worktree lease。

use std::collections::VecDeque;
use std::time::Duration;

use crate::studio::store::directory::DirectoryDelta;

/// 单批最多应用的 commit 数；一批共享一个 SQLite 事务。
pub(super) const MAX_BATCH_COMMITS: usize = 64;
/// 单批最多聚合的字节预算；超过即提前落库，避免长期占用内存。
pub(super) const MAX_BATCH_BYTES: usize = 4 * 1024 * 1024;
/// 队列条目数软上限；达到后停止进一步合并。
pub(super) const MAX_QUEUE_ENTRIES: usize = 4_096;
/// 队列保留字节软上限；达到后停止进一步合并。
pub(super) const MAX_QUEUE_BYTES: usize = 64 * 1024 * 1024;
/// 单条目录条目允许合并的最大字节。
pub(super) const MAX_MERGE_BYTES: usize = 512 * 1024;
/// 首条待写事实允许等待的最大批量时间窗口。
pub(super) const FLUSH_INTERVAL: Duration = Duration::from_secs(5);
/// 进入有界指数退避前允许的快速重试次数。
pub(super) const FAST_BATCH_RETRIES: usize = 3;
/// 瞬时失败的重试退避基值。
pub(super) const RETRY_BACKOFF: Duration = Duration::from_millis(100);
/// 退化后的最大自动重试间隔。
pub(super) const MAX_RETRY_BACKOFF: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
pub(super) enum StudioMutation {
    Directory(Box<StudioDirectoryMutation>),
}

#[derive(Debug, Clone)]
pub(super) enum StudioDirectoryMutation {
    Delta(DirectoryDelta),
    WorktreeLease(crate::studio::agent_host::worktree_lease::WorktreeLease),
}

#[derive(Debug, Clone)]
pub(super) struct QueuedMutation {
    ticket: u64,
    accepted_at: tokio::time::Instant,
    bytes: usize,
    pub(super) mutation: StudioMutation,
}

impl QueuedMutation {
    fn new(ticket: u64, mutation: StudioMutation) -> Self {
        let bytes = mutation.estimated_bytes();
        Self {
            ticket,
            accepted_at: tokio::time::Instant::now(),
            bytes,
            mutation,
        }
    }

    pub(super) const fn ticket(&self) -> u64 {
        self.ticket
    }

    pub(super) const fn bytes(&self) -> usize {
        self.bytes
    }
}

impl StudioMutation {
    /// 估算条目保留字节，用于队列与批次的压力预算。
    fn estimated_bytes(&self) -> usize {
        let StudioMutation::Directory(directory) = self;
        match directory.as_ref() {
            StudioDirectoryMutation::Delta(delta) => directory_bytes(delta),
            StudioDirectoryMutation::WorktreeLease(_) => 512,
        }
    }
}

fn directory_bytes(delta: &DirectoryDelta) -> usize {
    128 * (delta.session_activity.len()
        + delta.session_registrations.len()
        + delta.thread_upserts.len()
        + delta.unregistered_faults.len()
        + delta.thread_removals.len()
        + delta.project_upserts.len()
        + delta.project_removals.len())
}

pub(super) fn queue_directory(ticket: u64, delta: DirectoryDelta) -> QueuedMutation {
    QueuedMutation::new(
        ticket,
        StudioMutation::Directory(Box::new(StudioDirectoryMutation::Delta(delta))),
    )
}

pub(super) fn queue_worktree_lease(
    ticket: u64,
    lease: crate::studio::agent_host::worktree_lease::WorktreeLease,
) -> QueuedMutation {
    QueuedMutation::new(
        ticket,
        StudioMutation::Directory(Box::new(StudioDirectoryMutation::WorktreeLease(lease))),
    )
}

/// 只把新的目录 delta 合并进队尾最近一个 Delta 条目；顺序不变，字节有界。
///
/// 合并只更新内容、保留原 ticket：ticket 在推送时按序分配，队列中的 ticket 严格递增，
/// 这样 `durable_ticket` 才能作为真正的连续水位。
pub(super) fn try_coalesce_directory(
    queue: &mut VecDeque<QueuedMutation>,
    incoming: &DirectoryDelta,
) -> bool {
    let incoming_bytes = directory_bytes(incoming);
    let Some(entry) = queue.back_mut() else {
        return false;
    };
    if entry.bytes.saturating_add(incoming_bytes) > MAX_MERGE_BYTES {
        return false;
    }
    // `StudioMutation` 目前只有 `Directory` 一种变体：这里用不可反驳的模式绑定，避免
    // irrefutable let-else 的 unreachable else。将来新增变体时本行会变成编译错误，
    // 提醒恢复"非目录条目不可合并"的分支判断。
    let StudioMutation::Directory(existing) = &mut entry.mutation;
    let StudioDirectoryMutation::Delta(existing) = existing.as_mut() else {
        return false;
    };
    merge_directory_delta(existing, incoming);
    entry.bytes = entry.bytes.saturating_add(incoming_bytes);
    entry.accepted_at = tokio::time::Instant::now();
    true
}

fn merge_directory_delta(target: &mut DirectoryDelta, incoming: &DirectoryDelta) {
    target
        .session_activity
        .extend(incoming.session_activity.iter().cloned());
    target
        .session_registrations
        .extend(incoming.session_registrations.iter().cloned());
    target
        .thread_upserts
        .extend(incoming.thread_upserts.iter().cloned());
    target
        .unregistered_faults
        .extend(incoming.unregistered_faults.iter().cloned());
    target
        .thread_removals
        .extend(incoming.thread_removals.iter().cloned());
    target
        .project_upserts
        .extend(incoming.project_upserts.iter().cloned());
    target
        .project_removals
        .extend(incoming.project_removals.iter().cloned());
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct QueuePressure {
    pub(super) entries: usize,
    pub(super) bytes: usize,
    pub(super) oldest_accepted_at: Option<tokio::time::Instant>,
}

pub(super) fn queue_pressure(queue: &VecDeque<QueuedMutation>) -> QueuePressure {
    let mut pressure = QueuePressure {
        entries: queue.len(),
        ..QueuePressure::default()
    };
    for entry in queue {
        pressure.bytes = pressure.bytes.saturating_add(entry.bytes);
        pressure.oldest_accepted_at = Some(
            pressure
                .oldest_accepted_at
                .map_or(entry.accepted_at, |current| current.min(entry.accepted_at)),
        );
    }
    pressure
}

pub(super) struct PendingBatch {
    pub(super) entries: Vec<QueuedMutation>,
}

impl PendingBatch {
    pub(super) fn commit_count(&self) -> usize {
        self.entries.len()
    }

    pub(super) fn bytes(&self) -> usize {
        self.entries
            .iter()
            .fold(0usize, |total, entry| total.saturating_add(entry.bytes))
    }

    pub(super) fn max_ticket(&self) -> Option<u64> {
        self.entries.iter().map(QueuedMutation::ticket).max()
    }
}
