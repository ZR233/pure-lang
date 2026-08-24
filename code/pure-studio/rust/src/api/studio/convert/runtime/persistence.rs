use pl_studio_runtime::{PersistenceState, PersistenceStateSnapshot};

use crate::api::studio::types::{BridgePersistenceState, BridgePersistenceStateSnapshot};

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
