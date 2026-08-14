use std::sync::Arc;

use anyhow::Error;
use pl_protocol::{ObservedStateMeta, ObservedStatePhase, StateError, StateOperation};
use tokio::sync::{Mutex, RwLock};

use crate::{StudioLspHealth, StudioLspServer, StudioLspStateSnapshot, StudioProductEventRuntime};

/// LSP published state 的唯一 owner。
#[derive(Clone)]
pub(super) struct LspStateRuntime {
    command_lock: Arc<Mutex<()>>,
    state: Arc<RwLock<StudioLspStateSnapshot>>,
    events: StudioProductEventRuntime,
}

impl LspStateRuntime {
    pub(super) fn new(events: StudioProductEventRuntime) -> Self {
        Self {
            command_lock: Arc::new(Mutex::new(())),
            state: Arc::new(RwLock::new(StudioLspStateSnapshot {
                meta: ObservedStateMeta::uninitialized(unix_seconds()),
                health: empty_health(),
            })),
            events,
        }
    }

    pub(super) async fn command(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.command_lock.lock().await
    }

    pub(super) async fn read(&self) -> StudioLspStateSnapshot {
        self.state.read().await.clone()
    }

    pub(super) async fn begin(&self, operation: StateOperation) {
        let previous = self.read().await;
        let revision = previous.meta.revision.saturating_add(1);
        self.publish(StudioLspStateSnapshot {
            meta: ObservedStateMeta {
                revision,
                phase: ObservedStatePhase::Running {
                    operation,
                    operation_id: format!("lsp-{}-{revision}", operation_name(operation)),
                },
                updated_at: unix_seconds(),
                last_checked_at: previous.meta.last_checked_at,
                stale: previous.meta.stale,
            },
            health: previous.health,
        })
        .await;
    }

    pub(super) async fn ready(&self, health: StudioLspHealth, checked: bool) {
        let previous = self.read().await;
        let now = unix_seconds();
        self.publish(StudioLspStateSnapshot {
            meta: ObservedStateMeta {
                revision: previous.meta.revision.saturating_add(1),
                phase: ObservedStatePhase::Ready,
                updated_at: now,
                last_checked_at: if checked {
                    latest_checked_at(&health).or(Some(now))
                } else {
                    previous.meta.last_checked_at
                },
                stale: false,
            },
            health,
        })
        .await;
    }

    pub(super) async fn failed(&self, operation: StateOperation, error: &Error, checked: bool) {
        let previous = self.read().await;
        let now = unix_seconds();
        self.publish(StudioLspStateSnapshot {
            meta: ObservedStateMeta {
                revision: previous.meta.revision.saturating_add(1),
                phase: ObservedStatePhase::Failed {
                    operation,
                    error: StateError {
                        code: "lspOperationFailed".to_string(),
                        message: format!("{error:#}"),
                        retryable: true,
                    },
                },
                updated_at: now,
                last_checked_at: if checked {
                    Some(now)
                } else {
                    previous.meta.last_checked_at
                },
                stale: true,
            },
            health: previous.health,
        })
        .await;
    }

    pub(super) async fn refresh(&self, health: StudioLspHealth) {
        let previous = self.read().await;
        if matches!(
            previous.meta.phase,
            ObservedStatePhase::Running { .. } | ObservedStatePhase::Stopped
        ) || previous.health == health
        {
            return;
        }
        self.publish(StudioLspStateSnapshot {
            meta: ObservedStateMeta {
                revision: previous.meta.revision.saturating_add(1),
                phase: ObservedStatePhase::Ready,
                updated_at: unix_seconds(),
                last_checked_at: latest_checked_at(&health).or(previous.meta.last_checked_at),
                stale: false,
            },
            health,
        })
        .await;
    }

    pub(super) async fn stopped(&self) {
        let previous = self.read().await;
        self.publish(StudioLspStateSnapshot {
            meta: ObservedStateMeta {
                revision: previous.meta.revision.saturating_add(1),
                phase: ObservedStatePhase::Stopped,
                updated_at: unix_seconds(),
                last_checked_at: previous.meta.last_checked_at,
                stale: previous.meta.stale,
            },
            health: previous.health,
        })
        .await;
    }

    async fn publish(&self, snapshot: StudioLspStateSnapshot) {
        *self.state.write().await = snapshot.clone();
        self.events.emit_lsp_state(snapshot);
    }
}

pub(super) async fn health(registry: &pl_lsp::LspRuntimeRegistry) -> StudioLspHealth {
    let snapshots = registry.snapshots().await;
    let active_lsp_servers = registry.active_server_names().await;
    StudioLspHealth {
        lsp_servers: snapshots
            .into_iter()
            .map(|server| StudioLspServer {
                id: server.id,
                display_name: server.display_name,
                extensions: server.extensions,
                language_ids: server.language_ids,
                availability_kind: server.availability_kind.as_str().to_string(),
                availability_message: server.availability_message,
                last_checked_at: server.last_checked_at,
                diagnostic_count: server.diagnostic_count as u64,
                activity_kind: server.activity_kind.as_str().to_string(),
                activity_title: server.activity_title,
                activity_message: server.activity_message,
                activity_percentage: server.activity_percentage,
                last_error: server.last_error,
                last_error_at: server.last_error_at,
            })
            .collect(),
        active_lsp_servers,
    }
}

fn empty_health() -> StudioLspHealth {
    StudioLspHealth {
        lsp_servers: Vec::new(),
        active_lsp_servers: Vec::new(),
    }
}

fn latest_checked_at(health: &StudioLspHealth) -> Option<i64> {
    health
        .lsp_servers
        .iter()
        .filter_map(|server| server.last_checked_at)
        .max()
}

fn operation_name(operation: StateOperation) -> &'static str {
    match operation {
        StateOperation::Initialize => "initialize",
        StateOperation::Activate => "activate",
        StateOperation::Reload => "reload",
        StateOperation::Reconcile => "reconcile",
        StateOperation::Discover => "discover",
        StateOperation::Check => "check",
        StateOperation::Probe => "probe",
        StateOperation::Repair => "repair",
        StateOperation::Reset => "reset",
        StateOperation::Shutdown => "shutdown",
    }
}

fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}
