//! write-behind 队列的批量常量、typed mutation 条目、合并策略与批量分组。

use std::collections::VecDeque;
use std::time::Duration;

use pl_core::{PersistenceClass, ThreadCommit};
use tokio::sync::oneshot;

use crate::PureError;
use crate::studio::runtime::ModelPerformanceState;
use crate::studio::store::directory::DirectoryDelta;

/// 单批最多应用的 commit 数；一批共享一个 SQLite 事务。
pub(super) const MAX_BATCH_COMMITS: usize = 64;
/// 普通提交上限；其后的容量只供终态收束使用。
pub(super) const NORMAL_PENDING_COMMITS: usize = 768;
/// 总积压上限；达到后入队方等待 writer 追赶（背压而不是丢弃）。
pub(super) const MAX_PENDING_COMMITS: usize = 1024;
/// 首条待写事实允许等待的最大批量时间窗口。
pub(super) const FLUSH_INTERVAL: Duration = Duration::from_secs(5);
/// 进入公开 Degraded 状态前的快速重试次数。
pub(super) const FAST_BATCH_RETRIES: usize = 3;
/// 瞬时失败的重试退避基值。
pub(super) const RETRY_BACKOFF: Duration = Duration::from_millis(100);
/// Degraded 后的最大自动重试间隔。
pub(super) const MAX_RETRY_BACKOFF: Duration = Duration::from_secs(30);

/// 穷尽的 Studio typed persistence mutation。
///
/// Thread 包含 working object、Transcript、Timeline 与 Thread projection；
/// Directory 包含产品目录和其他有界 Studio object。
#[derive(Debug, Clone)]
pub(super) enum StudioMutation {
    Thread(Box<ThreadCommit>),
    Directory(Box<StudioDirectoryMutation>),
}

#[derive(Debug, Clone)]
pub(super) enum StudioDirectoryMutation {
    Delta(DirectoryDelta),
    ModelPerformance(ObservedStateCommit),
}

#[derive(Debug, Clone)]
pub(super) struct QueuedMutation {
    accepted_at: tokio::time::Instant,
    pub(super) mutation: StudioMutation,
}

impl QueuedMutation {
    fn new(mutation: StudioMutation) -> Self {
        Self {
            accepted_at: tokio::time::Instant::now(),
            mutation,
        }
    }
}

pub(super) enum QueueEntry {
    Mutation(QueuedMutation),
    Barrier(oneshot::Sender<Result<(), PureError>>),
}

#[derive(Debug, Clone)]
pub(super) struct ObservedStateCommit {
    pub(super) revision: u64,
    pub(super) value: ModelPerformanceState,
}

impl QueueEntry {
    pub(super) const fn is_commit(&self) -> bool {
        matches!(self, Self::Mutation(_))
    }

    pub(super) fn terminal_key(&self) -> Option<String> {
        match self {
            Self::Mutation(QueuedMutation {
                mutation: StudioMutation::Thread(commit),
                ..
            }) => terminal_turn_key(commit),
            Self::Mutation(_) | Self::Barrier(_) => None,
        }
    }

    /// worker panic 恢复只复制 typed mutation；barrier 由失败路径显式唤醒。
    pub(super) fn clone_commit(&self) -> Option<Self> {
        match self {
            Self::Mutation(commit) => Some(Self::Mutation(commit.clone())),
            Self::Barrier(_) => None,
        }
    }

    pub(super) fn flushes_immediately(&self) -> bool {
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

    pub(super) fn accepted_at(&self) -> Option<tokio::time::Instant> {
        match self {
            Self::Mutation(commit) => Some(commit.accepted_at),
            Self::Barrier(_) => None,
        }
    }

    pub(super) fn contains_directory_fact_for(&self, owner_id: &str) -> bool {
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

pub(super) fn queue_thread(commit: ThreadCommit) -> QueueEntry {
    QueueEntry::Mutation(QueuedMutation::new(StudioMutation::Thread(Box::new(
        commit,
    ))))
}

pub(super) fn queue_directory(delta: DirectoryDelta) -> QueueEntry {
    QueueEntry::Mutation(QueuedMutation::new(StudioMutation::Directory(Box::new(
        StudioDirectoryMutation::Delta(delta),
    ))))
}

pub(super) fn queue_model_performance(commit: ObservedStateCommit) -> QueueEntry {
    QueueEntry::Mutation(QueuedMutation::new(StudioMutation::Directory(Box::new(
        StudioDirectoryMutation::ModelPerformance(commit),
    ))))
}

pub(super) fn started_turn_key(commit: &ThreadCommit) -> Option<String> {
    commit.facts.runtime_events.iter().find_map(|event| {
        let pl_core::AgentRuntimeEventKind::TurnStarted { turn_id, .. } = &event.kind else {
            return None;
        };
        Some(format!("{}:{turn_id}", commit.agent_id))
    })
}

pub(super) fn terminal_turn_key(commit: &ThreadCommit) -> Option<String> {
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

pub(super) fn try_coalesce_tail(queue: &mut VecDeque<QueueEntry>, next: ThreadCommit) -> bool {
    let Some(QueueEntry::Mutation(QueuedMutation {
        mutation: StudioMutation::Thread(previous),
        ..
    })) = queue.back_mut()
    else {
        return false;
    };
    previous.coalesce(Box::new(next)).is_ok()
}

pub(super) fn try_coalesce_observed_state(
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

pub(super) struct PendingBatch {
    pub(super) entries: Vec<QueueEntry>,
}

impl PendingBatch {
    pub(super) fn commit_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.is_commit())
            .count()
    }
}
