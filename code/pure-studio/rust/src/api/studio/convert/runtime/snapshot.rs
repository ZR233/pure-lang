//! core lifecycle 快照桥接。

use crate::api::studio::types::*;
use pl_studio_runtime::*;

pub(crate) fn runtime_snapshot(snapshot: StudioRuntimeSnapshot) -> RuntimeSnapshot {
    RuntimeSnapshot {
        status: match snapshot.status {
            pl_studio_runtime::StudioRuntimeStatus::Uninitialized => {
                BridgeRuntimeStatus::Uninitialized
            }
            pl_studio_runtime::StudioRuntimeStatus::Initializing => {
                BridgeRuntimeStatus::Initializing
            }
            pl_studio_runtime::StudioRuntimeStatus::Ready => BridgeRuntimeStatus::Ready,
            pl_studio_runtime::StudioRuntimeStatus::ShuttingDown => {
                BridgeRuntimeStatus::ShuttingDown
            }
            pl_studio_runtime::StudioRuntimeStatus::Stopped => BridgeRuntimeStatus::Stopped,
            pl_studio_runtime::StudioRuntimeStatus::Failed => BridgeRuntimeStatus::Failed,
        },
        updated_at: snapshot.updated_at,
        error: snapshot.error,
    }
}
