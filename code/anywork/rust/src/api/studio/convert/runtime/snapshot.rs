//! core lifecycle 快照桥接。

use crate::api::studio::types::*;
use pl_studio_runtime::*;

pub(crate) fn runtime_snapshot(snapshot: StudioRuntimeSnapshot) -> RuntimeSnapshot {
    RuntimeSnapshot {
        revision: snapshot.revision,
        startup_recovery: snapshot
            .startup_recovery
            .map(|report| BridgeStartupRecovery {
                backup_path: report.backup_path,
                reason: report.reason,
                created_at: report.created_at,
            }),
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
                    error: state.error().into(),
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

/// Converts the runtime shutdown report into its FRB mirror without reinterpreting success.
pub(crate) fn bridge_shutdown_report(report: StudioShutdownReport) -> BridgeShutdownReport {
    BridgeShutdownReport {
        outcome: match report.outcome {
            StudioShutdownOutcome::NotStarted => BridgeShutdownOutcome::NotStarted,
            StudioShutdownOutcome::Clean => BridgeShutdownOutcome::Clean,
            StudioShutdownOutcome::Degraded => BridgeShutdownOutcome::Degraded,
        },
        issues: report
            .issues
            .into_iter()
            .map(|issue| BridgeShutdownIssue {
                stage: issue.stage,
                code: issue.code,
                message: issue.message,
                retryable: issue.retryable,
                correlation_id: issue.correlation_id,
            })
            .collect(),
        persistence: match report.persistence {
            StudioPendingPersistence::Unknown => BridgePendingPersistence::Unknown,
            StudioPendingPersistence::Pending { count } => {
                BridgePendingPersistence::Pending { count }
            }
            StudioPendingPersistence::Drained => BridgePendingPersistence::Drained,
        },
    }
}

/// Converts one bridge-side shutdown issue into the runtime protocol shape.
///
/// The two shapes are identical; the conversion only crosses the crate boundary so the bridge's
/// already-observed failures can seed the same runtime orchestration (single coordinator, no
/// post-hoc merge or second shutdown source of truth).
pub(crate) fn runtime_shutdown_issue(issue: BridgeShutdownIssue) -> StudioShutdownIssue {
    StudioShutdownIssue {
        stage: issue.stage,
        code: issue.code,
        message: issue.message,
        retryable: issue.retryable,
        correlation_id: issue.correlation_id,
    }
}
