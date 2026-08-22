use std::sync::Arc;

use anyhow::Error;
use pl_protocol::{
    ObservedResource, ObservedResourceCommand, ObservedResourceKind, StateError, StateOperation,
};
use tokio::sync::{Mutex, RwLock};

use crate::studio::ids::unix_seconds;
use crate::{
    LspAvailable, LspAvailableActivity, LspBusy, LspChecking, LspDisabled, LspIdle, LspIndexing,
    LspUnavailable, ProductEventBus, StudioLspHealth, StudioLspServer, StudioLspServerState,
    StudioLspStateSnapshot,
};

/// LSP published state 的唯一 owner。
#[derive(Clone)]
pub(super) struct LspStateRuntime {
    command_lock: Arc<Mutex<()>>,
    state: Arc<RwLock<StudioLspStateSnapshot>>,
    events: ProductEventBus,
}

impl LspStateRuntime {
    pub(super) fn new(events: ProductEventBus) -> Self {
        Self {
            command_lock: Arc::new(Mutex::new(())),
            state: Arc::new(RwLock::new(StudioLspStateSnapshot {
                state: ObservedResource::uninitialized(unix_seconds()),
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

    pub(super) async fn begin(&self, operation: StateOperation) -> anyhow::Result<()> {
        let previous = self.read().await;
        let revision = previous.state.revision();
        self.publish(StudioLspStateSnapshot {
            state: previous
                .state
                .decide(ObservedResourceCommand::Begin {
                    expected_revision: revision,
                    operation,
                    operation_id: format!(
                        "lsp-{}-{}",
                        operation_name(operation),
                        revision.saturating_add(1)
                    ),
                    started_at: unix_seconds(),
                })?
                .next_state,
        })
        .await;
        Ok(())
    }

    pub(super) async fn ready(&self, health: StudioLspHealth, checked: bool) -> anyhow::Result<()> {
        let previous = self.read().await;
        let now = unix_seconds();
        self.publish(StudioLspStateSnapshot {
            state: previous
                .state
                .decide(ObservedResourceCommand::Succeed {
                    expected_revision: previous.state.revision(),
                    updated_at: now,
                    last_checked_at: if checked {
                        latest_checked_at(&health).or(Some(now))
                    } else {
                        previous.state.last_checked_at()
                    },
                    value: health,
                })?
                .next_state,
        })
        .await;
        Ok(())
    }

    pub(super) async fn failed(&self, error: &Error) -> anyhow::Result<()> {
        let previous = self.read().await;
        self.publish(StudioLspStateSnapshot {
            state: previous
                .state
                .decide(ObservedResourceCommand::Fail {
                    expected_revision: previous.state.revision(),
                    failed_at: unix_seconds(),
                    error: StateError {
                        code: "lspOperationFailed".to_string(),
                        message: format!("{error:#}"),
                        retryable: true,
                    },
                })?
                .next_state,
        })
        .await;
        Ok(())
    }

    pub(super) async fn refresh(&self, health: StudioLspHealth) -> anyhow::Result<()> {
        let previous = self.read().await;
        if matches!(
            previous.state.kind(),
            ObservedResourceKind::Uninitialized
                | ObservedResourceKind::Loading
                | ObservedResourceKind::Refreshing
                | ObservedResourceKind::Failed
                | ObservedResourceKind::Stopped
        ) || previous.state.value() == Some(&health)
        {
            return Ok(());
        }
        self.publish(StudioLspStateSnapshot {
            state: previous
                .state
                .decide(ObservedResourceCommand::Observe {
                    expected_revision: previous.state.revision(),
                    observed_at: unix_seconds(),
                    last_checked_at: latest_checked_at(&health)
                        .or(previous.state.last_checked_at()),
                    value: health,
                })?
                .next_state,
        })
        .await;
        Ok(())
    }

    pub(super) async fn stopped(&self) -> anyhow::Result<()> {
        let previous = self.read().await;
        self.publish(StudioLspStateSnapshot {
            state: previous
                .state
                .decide(ObservedResourceCommand::Stop {
                    expected_revision: previous.state.revision(),
                    stopped_at: unix_seconds(),
                })?
                .next_state,
        })
        .await;
        Ok(())
    }

    async fn publish(&self, snapshot: StudioLspStateSnapshot) {
        *self.state.write().await = snapshot.clone();
        self.events.emit_lsp_state(snapshot);
    }
}

pub(super) async fn health(registry: &pl_lsp::LspRuntimeRegistry) -> StudioLspHealth {
    let snapshots = registry.snapshots().await;
    let active_lsp_servers = registry.active_server_names().await;
    let observed_at = unix_seconds();
    StudioLspHealth {
        lsp_servers: snapshots
            .into_iter()
            .map(|server| StudioLspServer {
                state: lsp_server_state(&server, observed_at),
                id: server.id,
                display_name: server.display_name,
                extensions: server.extensions,
                language_ids: server.language_ids,
            })
            .collect(),
        active_lsp_servers,
    }
}

fn latest_checked_at(health: &StudioLspHealth) -> Option<i64> {
    health
        .lsp_servers
        .iter()
        .filter_map(|server| match &server.state {
            StudioLspServerState::Available(state) => Some(state.checked_at()),
            StudioLspServerState::Unavailable(state) => Some(state.checked_at()),
            StudioLspServerState::Checking(_) | StudioLspServerState::Disabled(_) => None,
        })
        .max()
}

fn lsp_server_state(server: &pl_lsp::LspServerSnapshot, observed_at: i64) -> StudioLspServerState {
    match &server.availability_kind {
        pl_lsp::LspAvailabilityKind::Checking => StudioLspServerState::Checking(LspChecking::new(
            server
                .availability_message
                .as_deref()
                .unwrap_or("LSP health check is running"),
        )),
        pl_lsp::LspAvailabilityKind::Available => {
            let activity = match server.activity_kind {
                pl_lsp::LspActivityKind::Idle => LspAvailableActivity::Idle(LspIdle),
                pl_lsp::LspActivityKind::Busy => LspAvailableActivity::Busy(LspBusy::new(
                    server.activity_title.clone(),
                    server.activity_message.clone(),
                    server.activity_percentage,
                )),
                pl_lsp::LspActivityKind::Indexing => {
                    LspAvailableActivity::Indexing(LspIndexing::new(
                        server.activity_title.clone(),
                        server.activity_message.clone(),
                        server.activity_percentage,
                    ))
                }
            };
            StudioLspServerState::Available(LspAvailable::new(
                server.last_checked_at.unwrap_or(observed_at),
                server.diagnostic_count as u64,
                activity,
            ))
        }
        pl_lsp::LspAvailabilityKind::Disabled => StudioLspServerState::Disabled(LspDisabled::new(
            server
                .availability_message
                .as_deref()
                .unwrap_or("LSP server is disabled"),
        )),
        pl_lsp::LspAvailabilityKind::Unavailable => {
            unavailable_lsp_state(server, observed_at, "lspServerUnavailable")
        }
        pl_lsp::LspAvailabilityKind::MissingCommand => {
            unavailable_lsp_state(server, observed_at, "lspCommandMissing")
        }
        pl_lsp::LspAvailabilityKind::MissingServerComponent(_) => {
            unavailable_lsp_state(server, observed_at, "lspComponentMissing")
        }
    }
}

fn unavailable_lsp_state(
    server: &pl_lsp::LspServerSnapshot,
    observed_at: i64,
    code: &str,
) -> StudioLspServerState {
    StudioLspServerState::Unavailable(LspUnavailable::new(
        server
            .last_error_at
            .or(server.last_checked_at)
            .unwrap_or(observed_at),
        StateError {
            code: code.to_string(),
            message: server
                .last_error
                .clone()
                .or_else(|| server.availability_message.clone())
                .unwrap_or_else(|| "LSP server is unavailable".to_string()),
            retryable: true,
        },
    ))
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
