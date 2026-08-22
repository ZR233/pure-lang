//! core lifecycle 快照桥接。

use crate::api::studio::types::*;
use pl_studio_runtime::*;

pub(crate) fn runtime_snapshot(snapshot: StudioRuntimeSnapshot) -> RuntimeSnapshot {
    RuntimeSnapshot {
        revision: snapshot.revision,
        state: match snapshot.state {
            StudioRuntimeLifecycleState::Uninitialized(state) => {
                BridgeRuntimeState::Uninitialized(BridgeRuntimeTimestamp {
                    at: state.created_at(),
                })
            }
            StudioRuntimeLifecycleState::Initializing(state) => {
                BridgeRuntimeState::Initializing(BridgeRuntimeTimestamp {
                    at: state.started_at(),
                })
            }
            StudioRuntimeLifecycleState::Ready(state) => {
                BridgeRuntimeState::Ready(BridgeRuntimeTimestamp {
                    at: state.ready_at(),
                })
            }
            StudioRuntimeLifecycleState::ShuttingDown(state) => {
                BridgeRuntimeState::ShuttingDown(BridgeRuntimeTimestamp {
                    at: state.started_at(),
                })
            }
            StudioRuntimeLifecycleState::Stopped(state) => {
                BridgeRuntimeState::Stopped(BridgeRuntimeTimestamp {
                    at: state.stopped_at(),
                })
            }
            StudioRuntimeLifecycleState::Failed(state) => {
                BridgeRuntimeState::Failed(BridgeFailedRuntimeState {
                    failed_at: state.failed_at(),
                    error: BridgeStateError {
                        code: state.error().code.clone(),
                        message: state.error().message.clone(),
                        retryable: state.error().retryable,
                    },
                })
            }
        },
        active_turns: snapshot
            .active_turns
            .into_iter()
            .map(|turn| BridgeActiveTurn {
                thread_id: turn.thread_id,
                turn_id: turn.turn_id,
            })
            .collect(),
    }
}
