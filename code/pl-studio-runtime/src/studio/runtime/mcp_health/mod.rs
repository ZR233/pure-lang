use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use anyhow::Result;
use pl_protocol::{ObservedStateMeta, ObservedStatePhase, StateError, StateOperation};
use tokio::sync::{Mutex, RwLock, broadcast::error::RecvError};

use crate::config::{EffectiveMcpServerConfig, McpServerStatusKind, effective_mcp_servers};
use crate::mcp::{McpAvailabilityKind, McpAvailabilitySnapshot};
use crate::{StudioMcpHealth, StudioMcpServer, StudioMcpStateSnapshot};

use super::StudioRuntime;

/// MCP published state 的唯一 owner。
#[derive(Clone)]
pub(super) struct McpStateRuntime {
    command_lock: Arc<Mutex<()>>,
    state: Arc<RwLock<StudioMcpStateSnapshot>>,
    desired_effective_fingerprint: Arc<RwLock<Option<String>>>,
    applied_effective_fingerprint: Arc<RwLock<Option<String>>>,
}

struct McpReconcilePlan {
    _command_guard: tokio::sync::OwnedMutexGuard<()>,
    servers: BTreeMap<String, EffectiveMcpServerConfig>,
    effective_fingerprint: String,
    desired_public_fingerprint: String,
}

impl McpStateRuntime {
    pub(super) fn new() -> Self {
        Self {
            command_lock: Arc::new(Mutex::new(())),
            state: Arc::new(RwLock::new(StudioMcpStateSnapshot {
                meta: ObservedStateMeta::uninitialized(unix_seconds()),
                desired_config_fingerprint: String::new(),
                applied_config_fingerprint: String::new(),
                health: empty_health(),
            })),
            desired_effective_fingerprint: Arc::new(RwLock::new(None)),
            applied_effective_fingerprint: Arc::new(RwLock::new(None)),
        }
    }

    async fn read(&self) -> StudioMcpStateSnapshot {
        self.state.read().await.clone()
    }

    async fn publish(&self, snapshot: StudioMcpStateSnapshot) {
        *self.state.write().await = snapshot;
    }
}

impl StudioRuntime {
    pub async fn reconcile_mcp_runtime(&self) -> Result<()> {
        let Some(plan) = self.prepare_mcp_reconcile().await? else {
            return Ok(());
        };
        self.complete_mcp_reconcile(plan).await
    }

    pub(super) async fn start_mcp_reconcile_background(&self) -> Result<()> {
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

    pub(super) async fn stop_mcp_startup_reconcile(&self) {
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
        if effective_unchanged && matches!(previous.meta.phase, ObservedStatePhase::Ready) {
            return Ok(None);
        }
        let desired_public_fingerprint = self
            .desired_mcp_fingerprint(&previous, &effective_fingerprint, public_fingerprint)
            .await;
        self.publish_mcp_running(
            previous,
            desired_public_fingerprint.clone(),
            StateOperation::Reconcile,
            !effective_unchanged,
        )
        .await;
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
                self.publish_mcp_failed(StateOperation::Reconcile, &error)
                    .await;
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
        self.publish_mcp_running(
            previous,
            desired_public_fingerprint.clone(),
            StateOperation::Reset,
            true,
        )
        .await;
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
                self.publish_mcp_failed(StateOperation::Reset, &error).await;
                Err(error.into())
            }
        }
    }

    pub(super) async fn start_mcp_health_watcher(&self) {
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

    pub(super) async fn stop_mcp_health_watcher(&self) {
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

    pub(super) async fn publish_mcp_stopped(&self) {
        let previous = self.external_runtimes.mcp_state.read().await;
        let snapshot = StudioMcpStateSnapshot {
            meta: ObservedStateMeta {
                revision: previous.meta.revision.saturating_add(1),
                phase: ObservedStatePhase::Stopped,
                updated_at: unix_seconds(),
                last_checked_at: previous.meta.last_checked_at,
                stale: previous.meta.stale,
            },
            ..previous
        };
        self.publish_mcp(snapshot).await;
    }

    async fn desired_mcp_fingerprint(
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
            return previous.desired_config_fingerprint.clone();
        }
        *desired_effective = Some(effective_fingerprint.to_string());
        if public_fingerprint == previous.applied_config_fingerprint
            && !previous.applied_config_fingerprint.is_empty()
        {
            format!(
                "{public_fingerprint}:g{}",
                previous.meta.revision.saturating_add(1)
            )
        } else {
            public_fingerprint
        }
    }

    async fn publish_mcp_running(
        &self,
        previous: StudioMcpStateSnapshot,
        desired_config_fingerprint: String,
        operation: StateOperation,
        desired_differs_from_applied: bool,
    ) {
        let revision = previous.meta.revision.saturating_add(1);
        let snapshot = StudioMcpStateSnapshot {
            meta: ObservedStateMeta {
                revision,
                phase: ObservedStatePhase::Running {
                    operation,
                    operation_id: format!("mcp-{}-{revision}", operation_name(operation)),
                },
                updated_at: unix_seconds(),
                last_checked_at: previous.meta.last_checked_at,
                stale: desired_differs_from_applied,
            },
            desired_config_fingerprint,
            ..previous
        };
        self.publish_mcp(snapshot).await;
    }

    async fn publish_mcp_ready(&self, applied_config_fingerprint: String) -> Result<()> {
        let previous = self.external_runtimes.mcp_state.read().await;
        let health = self.collect_mcp_health().await?;
        let last_checked_at = health
            .mcp_servers
            .iter()
            .filter_map(|server| server.last_checked_at)
            .max();
        let snapshot = StudioMcpStateSnapshot {
            meta: ObservedStateMeta {
                revision: previous.meta.revision.saturating_add(1),
                phase: ObservedStatePhase::Ready,
                updated_at: unix_seconds(),
                last_checked_at,
                stale: false,
            },
            desired_config_fingerprint: applied_config_fingerprint.clone(),
            applied_config_fingerprint,
            health,
        };
        self.publish_mcp(snapshot).await;
        Ok(())
    }

    async fn publish_mcp_failed(&self, operation: StateOperation, error: &impl std::fmt::Display) {
        let previous = self.external_runtimes.mcp_state.read().await;
        let health = self
            .collect_mcp_health()
            .await
            .unwrap_or_else(|_| previous.health.clone());
        let last_checked_at = health
            .mcp_servers
            .iter()
            .filter_map(|server| server.last_checked_at)
            .max()
            .or(previous.meta.last_checked_at);
        let snapshot = StudioMcpStateSnapshot {
            meta: ObservedStateMeta {
                revision: previous.meta.revision.saturating_add(1),
                phase: ObservedStatePhase::Failed {
                    operation,
                    error: StateError {
                        code: "mcpOperationFailed".to_string(),
                        message: error.to_string(),
                        retryable: true,
                    },
                },
                updated_at: unix_seconds(),
                last_checked_at,
                stale: true,
            },
            health,
            ..previous
        };
        self.publish_mcp(snapshot).await;
    }

    async fn refresh_mcp_health_snapshot(&self) -> Result<()> {
        let previous = self.external_runtimes.mcp_state.read().await;
        if matches!(
            previous.meta.phase,
            ObservedStatePhase::Running { .. } | ObservedStatePhase::Stopped
        ) {
            return Ok(());
        }
        let health = self.collect_mcp_health().await?;
        if previous.health == health {
            return Ok(());
        }
        let snapshot = StudioMcpStateSnapshot {
            meta: ObservedStateMeta {
                revision: previous.meta.revision.saturating_add(1),
                phase: ObservedStatePhase::Ready,
                updated_at: unix_seconds(),
                last_checked_at: health
                    .mcp_servers
                    .iter()
                    .filter_map(|server| server.last_checked_at)
                    .max(),
                stale: previous.meta.stale,
            },
            health,
            ..previous
        };
        self.publish_mcp(snapshot).await;
        Ok(())
    }

    async fn publish_mcp(&self, snapshot: StudioMcpStateSnapshot) {
        self.external_runtimes
            .mcp_state
            .publish(snapshot.clone())
            .await;
        self.agent_facility.product_events.emit_mcp_state(snapshot);
    }

    async fn collect_mcp_health(&self) -> Result<StudioMcpHealth> {
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

fn mcp_health_from_effective(
    servers: BTreeMap<String, EffectiveMcpServerConfig>,
    snapshots: BTreeMap<String, McpAvailabilitySnapshot>,
    active_mcp_servers: Vec<String>,
) -> StudioMcpHealth {
    StudioMcpHealth {
        mcp_servers: servers
            .into_iter()
            .map(|(server_id, server)| {
                let snapshot = snapshots.get(&server_id);
                studio_mcp_server(server, snapshot)
            })
            .collect(),
        active_mcp_servers,
    }
}

fn studio_mcp_server(
    server: EffectiveMcpServerConfig,
    snapshot: Option<&McpAvailabilitySnapshot>,
) -> StudioMcpServer {
    let endpoint = server.config.endpoint_summary();
    StudioMcpServer {
        id: server.id,
        enabled: server.status_kind == McpServerStatusKind::Enabled,
        transport: server.config.transport.as_str().to_string(),
        endpoint,
        source_kind: server.source_kind.as_str().to_string(),
        status_kind: server.status_kind.as_str().to_string(),
        mutation_policy: server.mutation_policy.as_str().to_string(),
        availability_kind: snapshot
            .map(|snapshot| snapshot.availability_kind.as_str().to_string())
            .unwrap_or_else(|| fallback_availability_kind(server.status_kind).to_string()),
        availability_message: snapshot
            .and_then(|snapshot| snapshot.availability_message.clone())
            .or_else(|| fallback_availability_message(server.status_kind)),
        last_checked_at: snapshot.and_then(|snapshot| snapshot.last_checked_at),
        tool_count: snapshot
            .and_then(|snapshot| snapshot.tool_count)
            .map(|count| count as u64),
    }
}

fn effective_mcp_fingerprint(servers: &BTreeMap<String, EffectiveMcpServerConfig>) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for (server_id, server) in servers {
        server_id.hash(&mut hasher);
        server.config.hash(&mut hasher);
        server.status_kind.as_str().hash(&mut hasher);
        server.source_kind.as_str().hash(&mut hasher);
        server.bearer_token.hash(&mut hasher);
        server.tool_effect.hash(&mut hasher);
    }
    format!("{:x}", hasher.finish())
}

/// 公开 fingerprint 仅描述可安全公开的配置结构；credential 值只参与内部
/// effective fingerprint，确保轮换 credential 会 reconcile，却不会产生可关联的公开值。
fn public_mcp_fingerprint(servers: &BTreeMap<String, EffectiveMcpServerConfig>) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for (server_id, server) in servers {
        server_id.hash(&mut hasher);
        server.config.enabled.hash(&mut hasher);
        server.config.transport.hash(&mut hasher);
        server.config.command.hash(&mut hasher);
        server.config.args.len().hash(&mut hasher);
        server
            .config
            .env
            .keys()
            .for_each(|key| key.hash(&mut hasher));
        server.config.cwd.hash(&mut hasher);
        server.config.endpoint_summary().hash(&mut hasher);
        server.config.bearer_token_env_var.hash(&mut hasher);
        server
            .config
            .headers
            .keys()
            .for_each(|key| key.hash(&mut hasher));
        server.config.startup_timeout_secs.hash(&mut hasher);
        server.config.tool_timeout_secs.hash(&mut hasher);
        server.config.enabled_tools.hash(&mut hasher);
        server.config.disabled_tools.hash(&mut hasher);
        server.status_kind.as_str().hash(&mut hasher);
        server.source_kind.as_str().hash(&mut hasher);
        server.tool_effect.hash(&mut hasher);
    }
    format!("{:x}", hasher.finish())
}

fn empty_health() -> StudioMcpHealth {
    StudioMcpHealth {
        mcp_servers: Vec::new(),
        active_mcp_servers: Vec::new(),
    }
}

fn fallback_availability_kind(status: McpServerStatusKind) -> &'static str {
    match status {
        McpServerStatusKind::Enabled => McpAvailabilityKind::Checking.as_str(),
        McpServerStatusKind::Disabled => McpAvailabilityKind::Disabled.as_str(),
        McpServerStatusKind::MissingCredential => McpAvailabilityKind::MissingCredential.as_str(),
    }
}

fn fallback_availability_message(status: McpServerStatusKind) -> Option<String> {
    match status {
        McpServerStatusKind::Enabled => Some("MCP health check is running".to_string()),
        McpServerStatusKind::Disabled => {
            Some("MCP server is disabled in configuration".to_string())
        }
        McpServerStatusKind::MissingCredential => None,
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

fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        McpServerMutationPolicy, McpServerSourceKind, McpServerTransport, StudioMcpServerEntry,
    };

    #[test]
    fn public_fingerprint_is_not_derived_from_secret_values() {
        let first = servers_with_secret("first-secret");
        let second = servers_with_secret("second-secret");

        assert_ne!(
            effective_mcp_fingerprint(&first),
            effective_mcp_fingerprint(&second)
        );
        assert_eq!(
            public_mcp_fingerprint(&first),
            public_mcp_fingerprint(&second)
        );
    }

    fn servers_with_secret(secret: &str) -> BTreeMap<String, EffectiveMcpServerConfig> {
        let config = StudioMcpServerEntry {
            enabled: true,
            transport: McpServerTransport::Stdio,
            command: Some("mcp-server".to_string()),
            env: BTreeMap::from([("API_TOKEN".to_string(), secret.to_string())]),
            headers: BTreeMap::from([("Authorization".to_string(), secret.to_string())]),
            ..Default::default()
        };
        BTreeMap::from([(
            "server".to_string(),
            EffectiveMcpServerConfig {
                id: "server".to_string(),
                config,
                source_kind: McpServerSourceKind::BuiltIn,
                source_label: "Built-in".to_string(),
                source_detail: None,
                status_kind: McpServerStatusKind::Enabled,
                status_message: None,
                mutation_policy: McpServerMutationPolicy::LockedIdentity,
                bearer_token: Some(secret.to_string()),
                tool_effect: None,
            },
        )])
    }
}
