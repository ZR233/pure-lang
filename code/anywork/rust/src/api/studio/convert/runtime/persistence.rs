use pl_studio_runtime::{PersistenceState, PersistenceStateSnapshot};

use crate::api::studio::types::{
    BridgePersistenceQueueSnapshot, BridgePersistenceState, BridgePersistenceStateSnapshot,
    BridgeThreadPersistenceSnapshot,
};

use super::bridge_state_error;

pub(crate) fn bridge_persistence_state(
    snapshot: PersistenceStateSnapshot,
) -> BridgePersistenceStateSnapshot {
    let state = match snapshot.state {
        PersistenceState::Ready(state) => BridgePersistenceState::Ready {
            pending_commits: state.pending_commits,
        },
        PersistenceState::Flushing(state) => BridgePersistenceState::Flushing {
            pending_commits: state.pending_commits,
            oldest_pending_revision: state.oldest_pending_revision,
        },
        PersistenceState::Degraded(state) => BridgePersistenceState::Degraded {
            pending_commits: state.pending_commits,
            oldest_pending_revision: state.oldest_pending_revision,
            first_failed_at: state.first_failed_at,
            error: bridge_state_error(&state.error),
        },
        PersistenceState::Recovering(state) => BridgePersistenceState::Recovering {
            pending_commits: state.pending_commits,
            oldest_pending_revision: state.oldest_pending_revision,
            first_failed_at: state.first_failed_at,
        },
        PersistenceState::Blocked(state) => BridgePersistenceState::Blocked {
            pending_commits: state.pending_commits,
            oldest_pending_revision: state.oldest_pending_revision,
            first_failed_at: state.first_failed_at,
            error: bridge_state_error(&state.error),
        },
    };
    BridgePersistenceStateSnapshot {
        revision: snapshot.revision,
        state,
    }
}

/// 投影进程级持久化队列压力；每个字段都直接来自协调器观测，缺失保持 `None` 而非零。
///
/// 逐 Thread 水位沿用协调器的 `None`/`Some(0)` 区分：`None` 表示该 Thread 没有上报水位的
/// writer（未知），`Some(0)` 才是已观测到的零，桥接层不在这里折叠两者。
pub(crate) fn bridge_persistence_queue(
    snapshot: pl_protocol::PersistenceQueueSnapshot,
) -> BridgePersistenceQueueSnapshot {
    BridgePersistenceQueueSnapshot {
        pending_operations: snapshot.pending_operations,
        pending_bytes: snapshot.pending_bytes,
        in_flight_bytes: snapshot.in_flight_bytes,
        oldest_pending_age_millis: snapshot.oldest_pending_age_millis,
        last_error: snapshot.last_error,
        pressure_paused: snapshot.pressure_paused,
        threads: snapshot
            .threads
            .into_iter()
            .map(|thread| BridgeThreadPersistenceSnapshot {
                thread_id: thread.thread_id,
                state_dirty_revision: thread.state_dirty_revision,
                state_saving_revision: thread.state_saving_revision,
                state_durable_revision: thread.state_durable_revision,
                history_admitted_sequence: thread.history_admitted_sequence,
                history_durable_sequence: thread.history_durable_sequence,
                calls_admitted_sequence: thread.calls_admitted_sequence,
                calls_durable_sequence: thread.calls_durable_sequence,
                pending_operations: thread.pending_operations,
                pending_bytes: thread.pending_bytes,
                oldest_pending_age_millis: thread.oldest_pending_age_millis,
                in_flight_bytes: thread.in_flight_bytes,
                last_error: thread.last_error,
                pressure_paused: thread.pressure_paused,
            })
            .collect(),
    }
}
