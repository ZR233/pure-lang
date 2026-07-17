use std::collections::BTreeMap;

use crate::{StudioEventKind, StudioKeyValue, StudioMcpHealth, StudioMcpServer};
use anyhow::Result;
use tokio::sync::broadcast::error::RecvError;

use crate::config::{EffectiveMcpServerConfig, McpServerStatusKind, effective_mcp_servers};
use crate::mcp::{McpAvailabilityKind, McpAvailabilitySnapshot};

use super::StudioRuntime;

enum McpRuntimeRefresh {
    Reconcile,
    Recheck,
}

impl StudioRuntime {
    pub async fn reconcile_mcp_runtime(&self) -> Result<()> {
        self.refresh_mcp_runtime(McpRuntimeRefresh::Reconcile).await
    }

    pub async fn recheck_mcp_runtime(&self) -> Result<()> {
        self.refresh_mcp_runtime(McpRuntimeRefresh::Recheck).await
    }

    pub(super) async fn start_mcp_health_watcher(&self) {
        let mut watcher = self.mcp_health_watcher.lock().await;
        if watcher.as_ref().is_some_and(|handle| !handle.is_finished()) {
            return;
        }

        let runtime = self.clone();
        let mut updates = self.mcp_runtime.subscribe();
        *watcher = Some(tokio::spawn(async move {
            while let Ok(()) | Err(RecvError::Lagged(_)) = updates.recv().await {
                if let Err(error) = runtime.emit_mcp_health_snapshot().await {
                    eprintln!("[pl-core] failed to emit MCP health: {error:#}");
                }
            }
        }));
    }

    pub(super) async fn stop_mcp_health_watcher(&self) {
        if let Some(handle) = self.mcp_health_watcher.lock().await.take() {
            handle.abort();
        }
    }

    async fn refresh_mcp_runtime(&self, refresh: McpRuntimeRefresh) -> Result<()> {
        let config = self.config_store.load_or_default()?;
        let servers = effective_mcp_servers(&config);
        match refresh {
            McpRuntimeRefresh::Reconcile => self.mcp_runtime.reconcile(servers).await,
            McpRuntimeRefresh::Recheck => self.mcp_runtime.recheck(servers).await,
        }
        self.emit_mcp_health_snapshot().await
    }

    async fn emit_mcp_health_snapshot(&self) -> Result<()> {
        let health = self.mcp_health_snapshot().await?;
        self.events
            .emit(
                None,
                None,
                None,
                StudioEventKind::McpHealthChanged { health },
            )
            .await?;
        Ok(())
    }

    async fn mcp_health_snapshot(&self) -> Result<StudioMcpHealth> {
        let config = self.config_store.load_or_default()?;
        let servers = effective_mcp_servers(&config);
        let snapshots = self.mcp_runtime.snapshots().await;
        let active_mcp_servers = self.mcp_runtime.available_server_names().await;
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
        command: server.config.command,
        args: server.config.args,
        env: key_values(server.config.env),
        cwd: server.config.cwd,
        url: server.config.url,
        bearer_token_env_var: server.config.bearer_token_env_var,
        headers: key_values(server.config.headers),
        endpoint,
        source_kind: server.source_kind.as_str().to_string(),
        source_label: server.source_label,
        source_detail: server.source_detail,
        status_kind: server.status_kind.as_str().to_string(),
        status_message: server.status_message,
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

fn key_values(values: BTreeMap<String, String>) -> Vec<StudioKeyValue> {
    values
        .into_iter()
        .map(|(key, value)| StudioKeyValue { key, value })
        .collect()
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

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use crate::config::{
        McpServerTransport, StudioBuiltinMcpServerState, StudioConfig, StudioMcpServerEntry,
    };
    use crate::mcp::McpAvailabilitySnapshot;

    use super::*;

    #[test]
    fn health_snapshot_preserves_builtin_identity_metadata() {
        let server_id = crate::config::builtin_mcp_server_ids()[0];
        let mut config = StudioConfig::default_config();
        config.mcp.builtin_servers = BTreeMap::from([(
            server_id.to_string(),
            StudioBuiltinMcpServerState { enabled: false },
        )]);
        let health =
            mcp_health_from_effective(effective_mcp_servers(&config), BTreeMap::new(), Vec::new());

        let server = health
            .mcp_servers
            .iter()
            .find(|server| server.id == server_id)
            .unwrap();

        assert!(!server.enabled);
        assert_eq!(server.source_kind, "builtIn");
        assert_eq!(server.source_detail.as_deref(), Some("Zhipu Coding Plan"));
        assert_eq!(server.mutation_policy, "lockedIdentity");
        assert_eq!(server.availability_kind, "disabled");
    }

    #[test]
    fn health_snapshot_uses_registry_availability_when_present() {
        let mut config = StudioConfig::default_config();
        config.mcp.servers = BTreeMap::from([(
            "local_docs".to_string(),
            StudioMcpServerEntry {
                transport: McpServerTransport::StreamableHttp,
                url: Some("http://127.0.0.1:9/mcp".to_string()),
                ..Default::default()
            },
        )]);
        let health = mcp_health_from_effective(
            effective_mcp_servers(&config),
            BTreeMap::from([(
                "local_docs".to_string(),
                McpAvailabilitySnapshot {
                    server_id: "local_docs".to_string(),
                    availability_kind: McpAvailabilityKind::Available,
                    availability_message: Some("Available with 2 tools".to_string()),
                    last_checked_at: Some(123),
                    tool_count: Some(2),
                },
            )]),
            vec!["local_docs".to_string()],
        );

        let server = health
            .mcp_servers
            .iter()
            .find(|server| server.id == "local_docs")
            .unwrap();

        assert!(server.enabled);
        assert_eq!(server.availability_kind, "available");
        assert_eq!(
            server.availability_message.as_deref(),
            Some("Available with 2 tools")
        );
        assert_eq!(server.last_checked_at, Some(123));
        assert_eq!(server.tool_count, Some(2));
        assert_eq!(health.active_mcp_servers, vec!["local_docs".to_string()]);
    }
}
