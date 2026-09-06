//! PersistenceState 的计算与发布，以及 writer 退出时的屏障失败处理。

use std::collections::VecDeque;

use pl_protocol::StateError;

use crate::PureError;
use crate::studio::{
    BlockedPersistence, DegradedPersistence, FlushingPersistence, PersistenceState,
    PersistenceStateSnapshot, ReadyPersistence, RecoveringPersistence, unix_seconds,
};

use super::super::store_error;
use super::handle::WriterShared;
use super::queue::{QueueEntry, QueuedMutation, StudioDirectoryMutation, StudioMutation};

pub(super) fn update_healthy_state(shared: &WriterShared, pending: usize) {
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

pub(super) fn update_after_success(shared: &WriterShared, pending: usize, was_unhealthy: bool) {
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

pub(super) fn publish_degraded(shared: &WriterShared, error: &PureError) {
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

pub(super) fn publish_blocked(shared: &WriterShared, reason: &str) {
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
            QueueEntry::Mutation(QueuedMutation {
                mutation: StudioMutation::Thread(commit),
                ..
            }) => Some(commit.facts.revision),
            QueueEntry::Mutation(QueuedMutation {
                mutation: StudioMutation::Directory(directory),
                ..
            }) => match directory.as_ref() {
                StudioDirectoryMutation::ModelPerformance(commit) => Some(commit.revision),
                StudioDirectoryMutation::Delta(_) | StudioDirectoryMutation::Attachments(_) => None,
            },
            QueueEntry::Barrier(_) => None,
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
pub(super) fn fail_barriers(shared: &WriterShared, reason: &str) {
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
