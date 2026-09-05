//! MCP published state 的 owner 与 publish_* 状态发布。

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::Result;
use pl_protocol::{ObservedResource, ObservedResourceCommand, StateError, StateOperation};
use tokio::sync::{Mutex, RwLock};

use crate::config::EffectiveMcpServerConfig;
use crate::studio::ids::unix_seconds;
use crate::{StudioMcpStateData, StudioMcpStateSnapshot};

use super::super::StudioRuntime;
use super::health::mcp_server_checked_at;

/// MCP published state 的唯一 owner。
#[derive(Clone)]
pub(in crate::studio::runtime) struct McpStateRuntime {
    pub(super) command_lock: Arc<Mutex<()>>,
    state: Arc<RwLock<StudioMcpStateSnapshot>>,
    desired_effective_fingerprint: Arc<RwLock<Option<String>>>,
    pub(super) applied_effective_fingerprint: Arc<RwLock<Option<String>>>,
}

pub(super) struct McpReconcilePlan {
    pub(super) _command_guard: tokio::sync::OwnedMutexGuard<()>,
    pub(super) servers: BTreeMap<String, EffectiveMcpServerConfig>,
    pub(super) effective_fingerprint: String,
    pub(super) desired_public_fingerprint: String,
}

impl McpStateRuntime {
    pub(in crate::studio::runtime) fn new() -> Self {
        Self {
            command_lock: Arc::new(Mutex::new(())),
            state: Arc::new(RwLock::new(StudioMcpStateSnapshot {
                state: ObservedResource::uninitialized(unix_seconds()),
            })),
            desired_effective_fingerprint: Arc::new(RwLock::new(None)),
            applied_effective_fingerprint: Arc::new(RwLock::new(None)),
        }
    }

    pub(super) async fn read(&self) -> StudioMcpStateSnapshot {
        self.state.read().await.clone()
    }

    async fn publish(&self, snapshot: StudioMcpStateSnapshot) {
        *self.state.write().await = snapshot;
    }
}

impl StudioRuntime {
    pub(in crate::studio::runtime) async fn publish_mcp_stopped(&self) -> Result<()> {
        let previous = self.external_runtimes.mcp_state.read().await;
        let snapshot = StudioMcpStateSnapshot {
            state: previous
                .state
                .decide(ObservedResourceCommand::Stop {
                    expected_revision: previous.state.revision(),
                    stopped_at: unix_seconds(),
                })?
                .next_state,
        };
        self.publish_mcp(snapshot).await;
        Ok(())
    }

    pub(super) async fn desired_mcp_fingerprint(
        &self,
        previous: &StudioMcpStateSnapshot,
        effective_fingerprint: &str,
        public_fingerprint: String,
    ) -> String {
        let mut desired_effective = self
            .external_runtimes
            .mcp_state
            .desired_effective_fingerprint
            .write()
            .await;
        if desired_effective.as_deref() == Some(effective_fingerprint) {
            return previous
                .state
                .value()
                .map(|data| data.desired_config_fingerprint.clone())
                .unwrap_or_default();
        }
        *desired_effective = Some(effective_fingerprint.to_string());
        let applied_fingerprint = previous
            .state
            .value()
            .map(|data| data.applied_config_fingerprint.as_str())
            .unwrap_or_default();
        if public_fingerprint == applied_fingerprint && !applied_fingerprint.is_empty() {
            format!(
                "{public_fingerprint}:g{}",
                previous.state.revision().saturating_add(1)
            )
        } else {
            public_fingerprint
        }
    }

    pub(super) async fn publish_mcp_running(
        &self,
        previous: StudioMcpStateSnapshot,
        operation: StateOperation,
    ) -> Result<()> {
        let revision = previous.state.revision();
        let snapshot = StudioMcpStateSnapshot {
            state: previous
                .state
                .decide(ObservedResourceCommand::Begin {
                    expected_revision: revision,
                    operation,
                    operation_id: format!(
                        "mcp-{}-{}",
                        operation_name(operation),
                        revision.saturating_add(1)
                    ),
                    started_at: unix_seconds(),
                })?
                .next_state,
        };
        self.publish_mcp(snapshot).await;
        Ok(())
    }

    pub(super) async fn publish_mcp_ready(&self, applied_config_fingerprint: String) -> Result<()> {
        let previous = self.external_runtimes.mcp_state.read().await;
        let health = self.collect_mcp_health().await?;
        let last_checked_at = health
            .mcp_servers
            .iter()
            .filter_map(mcp_server_checked_at)
            .max();
        let snapshot = StudioMcpStateSnapshot {
            state: previous
                .state
                .decide(ObservedResourceCommand::Succeed {
                    expected_revision: previous.state.revision(),
                    updated_at: unix_seconds(),
                    last_checked_at,
                    value: StudioMcpStateData {
                        desired_config_fingerprint: applied_config_fingerprint.clone(),
                        applied_config_fingerprint,
                        health,
                    },
                })?
                .next_state,
        };
        self.publish_mcp(snapshot).await;
        Ok(())
    }

    pub(super) async fn publish_mcp_failed(&self, error: &impl std::fmt::Display) -> Result<()> {
        let previous = self.external_runtimes.mcp_state.read().await;
        let snapshot = StudioMcpStateSnapshot {
            state: previous
                .state
                .decide(ObservedResourceCommand::Fail {
                    expected_revision: previous.state.revision(),
                    failed_at: unix_seconds(),
                    error: StateError {
                        code: "mcpOperationFailed".to_string(),
                        message: error.to_string(),
                        retryable: true,
                    },
                })?
                .next_state,
        };
        self.publish_mcp(snapshot).await;
        Ok(())
    }

    pub(super) async fn publish_mcp(&self, snapshot: StudioMcpStateSnapshot) {
        self.external_runtimes
            .mcp_state
            .publish(snapshot.clone())
            .await;
        self.agent_facility.product_events.emit_mcp_state(snapshot);
    }
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
