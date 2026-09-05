//! MCP reconcile / reset 编排与后台健康 watcher。

use anyhow::Result;
use pl_protocol::{ObservedResourceCommand, ObservedResourceKind, StateOperation};
use tokio::sync::broadcast::error::RecvError;

use crate::config::effective_mcp_servers;
use crate::studio::ids::unix_seconds;
use crate::{StudioMcpHealth, StudioMcpStateSnapshot};

use super::super::StudioRuntime;
use super::fingerprint::{effective_mcp_fingerprint, public_mcp_fingerprint};
use super::health::{mcp_health_from_effective, mcp_server_checked_at};
use super::state::McpReconcilePlan;

impl StudioRuntime {
    pub async fn reconcile_mcp_runtime(&self) -> Result<()> {
        let Some(plan) = self.prepare_mcp_reconcile().await? else {
            return Ok(());
        };
        self.complete_mcp_reconcile(plan).await
    }

    pub(in crate::studio::runtime) async fn start_mcp_reconcile_background(&self) -> Result<()> {
        let mut task = self.external_runtimes.mcp_startup_reconcile.lock().await;
        if task.as_ref().is_some_and(|handle| !handle.is_finished()) {
            return Ok(());
        }
        let Some(plan) = self.prepare_mcp_reconcile().await? else {
            task.take();
            return Ok(());
        };
        let runtime = self.clone();
        *task = Some(tokio::spawn(async move {
            if let Err(error) = runtime.complete_mcp_reconcile(plan).await {
                tracing::warn!(
                    error_bytes = error.to_string().len(),
                    "background MCP startup reconcile failed"
                );
            }
        }));
        Ok(())
    }

    pub(in crate::studio::runtime) async fn stop_mcp_startup_reconcile(&self) {
        let task = self
            .external_runtimes
            .mcp_startup_reconcile
            .lock()
            .await
            .take();
        if let Some(task) = task {
            task.abort();
            let _ = task.await;
        }
    }

    async fn prepare_mcp_reconcile(&self) -> Result<Option<McpReconcilePlan>> {
        let config = self.config_runtime.read()?.config;
        let servers = effective_mcp_servers(&config);
        let effective_fingerprint = effective_mcp_fingerprint(&servers);
        let public_fingerprint = public_mcp_fingerprint(&servers);
        let command_guard = self
            .external_runtimes
            .mcp_state
            .command_lock
            .clone()
            .lock_owned()
            .await;
        let previous = self.external_runtimes.mcp_state.read().await;
        let effective_unchanged = self
            .external_runtimes
            .mcp_state
            .applied_effective_fingerprint
            .read()
            .await
            .as_ref()
            == Some(&effective_fingerprint);
        if effective_unchanged && previous.state.kind() == ObservedResourceKind::Ready {
            return Ok(None);
        }
        let desired_public_fingerprint = self
            .desired_mcp_fingerprint(&previous, &effective_fingerprint, public_fingerprint)
            .await;
        self.publish_mcp_running(previous, StateOperation::Reconcile)
            .await?;
        Ok(Some(McpReconcilePlan {
            _command_guard: command_guard,
            servers,
            effective_fingerprint,
            desired_public_fingerprint,
        }))
    }

    async fn complete_mcp_reconcile(&self, plan: McpReconcilePlan) -> Result<()> {
        let McpReconcilePlan {
            _command_guard,
            servers,
            effective_fingerprint,
            desired_public_fingerprint,
        } = plan;
        match self.external_runtimes.mcp.reconcile(servers).await {
            Ok(()) => {
                *self
                    .external_runtimes
                    .mcp_state
                    .applied_effective_fingerprint
                    .write()
                    .await = Some(effective_fingerprint);
                self.publish_mcp_ready(desired_public_fingerprint).await?;
                Ok(())
            }
            Err(error) => {
                self.publish_mcp_failed(&error).await?;
                Err(error.into())
            }
        }
    }

    pub async fn reset_mcp(&self, request: pl_protocol::studio::McpResetRequest) -> Result<()> {
        let scope = match request {
            pl_protocol::studio::McpResetRequest::Server { server_id } => {
                crate::McpResetScope::Server { server_id }
            }
            pl_protocol::studio::McpResetRequest::All => crate::McpResetScope::All,
        };
        let config = self.config_runtime.read()?.config;
        let servers = effective_mcp_servers(&config);
        let effective_fingerprint = effective_mcp_fingerprint(&servers);
        let public_fingerprint = public_mcp_fingerprint(&servers);
        let _command = self.external_runtimes.mcp_state.command_lock.lock().await;
        let previous = self.external_runtimes.mcp_state.read().await;
        let desired_public_fingerprint = self
            .desired_mcp_fingerprint(&previous, &effective_fingerprint, public_fingerprint)
            .await;
        self.publish_mcp_running(previous, StateOperation::Reset)
            .await?;
        match self.external_runtimes.mcp.reset(scope, servers).await {
            Ok(()) => {
                *self
                    .external_runtimes
                    .mcp_state
                    .applied_effective_fingerprint
                    .write()
                    .await = Some(effective_fingerprint);
                self.publish_mcp_ready(desired_public_fingerprint).await?;
                Ok(())
            }
            Err(error) => {
                self.publish_mcp_failed(&error).await?;
                Err(error.into())
            }
        }
    }

    pub(in crate::studio::runtime) async fn start_mcp_health_watcher(&self) {
        let mut watcher = self.external_runtimes.mcp_health_watcher.lock().await;
        if watcher.as_ref().is_some_and(|handle| !handle.is_finished()) {
            return;
        }

        let runtime = self.clone();
        let mut updates = self.external_runtimes.mcp.subscribe();
        *watcher = Some(tokio::spawn(async move {
            while let Ok(()) | Err(RecvError::Lagged(_)) = updates.recv().await {
                if let Err(error) = runtime.refresh_mcp_health_snapshot().await {
                    tracing::warn!(
                        error_bytes = error.to_string().len(),
                        "failed to publish MCP state snapshot"
                    );
                }
            }
        }));
    }

    pub(in crate::studio::runtime) async fn stop_mcp_health_watcher(&self) {
        if let Some(handle) = self
            .external_runtimes
            .mcp_health_watcher
            .lock()
            .await
            .take()
        {
            handle.abort();
        }
    }

    pub async fn read_mcp_state(&self) -> Result<StudioMcpStateSnapshot> {
        Ok(self.external_runtimes.mcp_state.read().await)
    }

    async fn refresh_mcp_health_snapshot(&self) -> Result<()> {
        let previous = self.external_runtimes.mcp_state.read().await;
        let Some(mut data) = previous.state.value().cloned() else {
            return Ok(());
        };
        let health = self.collect_mcp_health().await?;
        if data.health == health {
            return Ok(());
        }
        let last_checked_at = health
            .mcp_servers
            .iter()
            .filter_map(mcp_server_checked_at)
            .max()
            .or(previous.state.last_checked_at());
        data.health = health;
        let command = match previous.state.kind() {
            ObservedResourceKind::Ready => ObservedResourceCommand::Observe {
                expected_revision: previous.state.revision(),
                observed_at: unix_seconds(),
                last_checked_at,
                value: data,
            },
            ObservedResourceKind::Stale => ObservedResourceCommand::MarkStale {
                expected_revision: previous.state.revision(),
                stale_at: unix_seconds(),
                value: data,
            },
            ObservedResourceKind::Uninitialized
            | ObservedResourceKind::Loading
            | ObservedResourceKind::Refreshing
            | ObservedResourceKind::Degraded
            | ObservedResourceKind::Failed
            | ObservedResourceKind::Stopped => return Ok(()),
        };
        let snapshot = StudioMcpStateSnapshot {
            state: previous.state.decide(command)?.next_state,
        };
        self.publish_mcp(snapshot).await;
        Ok(())
    }

    pub(super) async fn collect_mcp_health(&self) -> Result<StudioMcpHealth> {
        let config = self.config_runtime.read()?.config;
        let servers = effective_mcp_servers(&config);
        let snapshots = self.external_runtimes.mcp.snapshots().await;
        let active_mcp_servers = self.external_runtimes.mcp.available_server_names().await;
        Ok(mcp_health_from_effective(
            servers,
            snapshots,
            active_mcp_servers,
        ))
    }
}
