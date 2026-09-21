//! PersistenceState 的计算与发布。

use pl_protocol::StateError;

use crate::PureError;
use crate::studio::{
    BlockedPersistence, DegradedPersistence, FlushingPersistence, PersistenceState,
    PersistenceStateSnapshot, ReadyPersistence, RecoveringPersistence, unix_seconds,
};

use super::handle::WriterShared;

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

/// 目录事实与 lease 没有单条 revision 概念；保留该位以维持状态快照契约。
fn oldest_pending_revision(_shared: &WriterShared) -> Option<u64> {
    None
}

fn pending_from_queue(shared: &WriterShared) -> usize {
    shared
        .queue
        .lock()
        .expect("write-behind queue lock poisoned")
        .len()
}
