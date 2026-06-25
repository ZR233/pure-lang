use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use pl_protocol::{PureError, Result};
use tokio::sync::{Mutex, broadcast};

use crate::config::{EffectiveMcpServerConfig, McpServerStatusKind};
use crate::tool::Tool;

use super::client::McpClient;
use super::tool_adapter::McpToolAdapter;
use super::transport::{McpProbeSuccess, PROBE_TIMEOUT, probe_server};
use super::wire::McpToolDefinition;

/// MCP server 的运行时可用状态。
///
/// 该状态只存在于进程内 registry，用来区分用户启用意图和当前真实可调用能力。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpAvailabilityKind {
    Checking,
    Available,
    Unavailable,
    Disabled,
    MissingCredential,
}

impl McpAvailabilityKind {
    pub fn as_str(self) -> &'static str {
        match self {
            McpAvailabilityKind::Checking => "checking",
            McpAvailabilityKind::Available => "available",
            McpAvailabilityKind::Unavailable => "unavailable",
            McpAvailabilityKind::Disabled => "disabled",
            McpAvailabilityKind::MissingCredential => "missingCredential",
        }
    }
}

/// Studio 展示用的 MCP availability 快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpAvailabilitySnapshot {
    pub server_id: String,
    pub availability_kind: McpAvailabilityKind,
    pub availability_message: Option<String>,
    pub last_checked_at: Option<i64>,
    pub tool_count: Option<usize>,
}

/// MCP runtime registry。
///
/// Registry 在后台探测配置启用且凭据完整的 MCP server，并缓存已初始化 client 与
/// `tools/list` 结果。对话与 subagent runner 只从这里读取当前 `available` tools，
/// 不在发送路径同步连接远端 MCP server。
#[derive(Clone)]
pub struct McpRuntimeRegistry {
    pub(super) state: Arc<Mutex<McpRuntimeRegistryState>>,
    updates: broadcast::Sender<()>,
}

impl fmt::Debug for McpRuntimeRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("McpRuntimeRegistry").finish_non_exhaustive()
    }
}

impl Default for McpRuntimeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl McpRuntimeRegistry {
    pub fn new() -> Self {
        let (updates, _) = broadcast::channel(64);
        Self {
            state: Arc::new(Mutex::new(McpRuntimeRegistryState::default())),
            updates,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.updates.subscribe()
    }

    pub async fn reconcile(&self, servers: BTreeMap<String, EffectiveMcpServerConfig>) {
        self.reconcile_with_policy(servers, ProbePolicy::ChangedOrUnavailable)
            .await;
    }

    pub async fn recheck(&self, servers: BTreeMap<String, EffectiveMcpServerConfig>) {
        self.reconcile_with_policy(servers, ProbePolicy::Force)
            .await;
    }

    pub async fn snapshots(&self) -> BTreeMap<String, McpAvailabilitySnapshot> {
        self.state
            .lock()
            .await
            .servers
            .iter()
            .map(|(server_id, state)| {
                (
                    server_id.clone(),
                    McpAvailabilitySnapshot {
                        server_id: server_id.clone(),
                        availability_kind: state.availability_kind,
                        availability_message: state.availability_message.clone(),
                        last_checked_at: state.last_checked_at,
                        tool_count: match state.availability_kind {
                            McpAvailabilityKind::Available => Some(state.tools.len()),
                            McpAvailabilityKind::Checking
                            | McpAvailabilityKind::Unavailable
                            | McpAvailabilityKind::Disabled
                            | McpAvailabilityKind::MissingCredential => None,
                        },
                    },
                )
            })
            .collect()
    }

    pub async fn available_server_names(&self) -> Vec<String> {
        self.state
            .lock()
            .await
            .servers
            .iter()
            .filter(|(_, state)| state.availability_kind == McpAvailabilityKind::Available)
            .map(|(server_id, _)| server_id.clone())
            .collect()
    }

    pub async fn register_available_tools(&self, core: &mut crate::PureCore) -> Result<()> {
        for available in self.available_servers().await {
            for definition in available.tools {
                let adapter = McpToolAdapter::new(
                    &available.server_id,
                    definition,
                    available.client.clone(),
                    Some(self.clone()),
                )?;
                if core.has_tool(adapter.name()) {
                    return Err(PureError::ConfigError(format!(
                        "mcp tool '{}' conflicts with an existing tool",
                        adapter.name()
                    )));
                }
                core.register_tool(adapter);
            }
        }
        Ok(())
    }

    async fn reconcile_with_policy(
        &self,
        servers: BTreeMap<String, EffectiveMcpServerConfig>,
        policy: ProbePolicy,
    ) {
        let server_ids = servers.keys().cloned().collect::<BTreeSet<_>>();
        let mut probes = Vec::new();
        let mut changed = false;
        let mut shutdown_clients = Vec::new();
        {
            let mut state = self.state.lock().await;
            let before_len = state.servers.len();
            state.servers.retain(|server_id, server| {
                let keep = server_ids.contains(server_id);
                if !keep && let Some(client) = server.client.take() {
                    shutdown_clients.push(client);
                }
                keep
            });
            changed |= before_len != state.servers.len();

            for (server_id, server) in servers {
                let fingerprint = server_fingerprint(&server);
                match server.status_kind {
                    McpServerStatusKind::Enabled => {
                        let should_probe =
                            should_probe_server(state.servers.get(&server_id), fingerprint, policy);
                        if should_probe {
                            if let Some(mut previous) = state.servers.insert(
                                server_id.clone(),
                                McpRuntimeServerState::checking(fingerprint),
                            ) && let Some(client) = previous.client.take()
                            {
                                shutdown_clients.push(client);
                            }
                            probes.push((server_id, server, fingerprint));
                            changed = true;
                        } else if let std::collections::btree_map::Entry::Vacant(entry) =
                            state.servers.entry(server_id)
                        {
                            entry.insert(McpRuntimeServerState::checking(fingerprint));
                            changed = true;
                        }
                    }
                    McpServerStatusKind::Disabled => {
                        changed |= insert_terminal_state(
                            &mut state,
                            server_id,
                            McpRuntimeServerState::disabled(fingerprint),
                            &mut shutdown_clients,
                        );
                    }
                    McpServerStatusKind::MissingCredential => {
                        changed |= insert_terminal_state(
                            &mut state,
                            server_id,
                            McpRuntimeServerState::missing_credential(
                                fingerprint,
                                server.status_message,
                            ),
                            &mut shutdown_clients,
                        );
                    }
                }
            }
        }
        for client in shutdown_clients {
            client.shutdown().await;
        }
        if changed {
            self.emit_update();
        }
        for (server_id, server, fingerprint) in probes {
            let registry = self.clone();
            tokio::spawn(async move {
                let checked_at = unix_seconds();
                let result =
                    tokio::time::timeout(PROBE_TIMEOUT, probe_server(&server_id, &server)).await;
                registry
                    .store_probe_result(server_id, fingerprint, checked_at, result)
                    .await;
            });
        }
    }

    async fn available_servers(&self) -> Vec<McpAvailableServer> {
        self.state
            .lock()
            .await
            .servers
            .iter()
            .filter_map(|(server_id, state)| {
                if state.availability_kind != McpAvailabilityKind::Available {
                    return None;
                }
                Some(McpAvailableServer {
                    server_id: server_id.clone(),
                    client: state.client.as_ref()?.clone(),
                    tools: state.tools.clone(),
                })
            })
            .collect()
    }

    async fn store_probe_result(
        &self,
        server_id: String,
        fingerprint: u64,
        checked_at: i64,
        result: std::result::Result<Result<McpProbeSuccess>, tokio::time::error::Elapsed>,
    ) {
        let next = match result {
            Ok(Ok(success)) => McpRuntimeServerState::available(
                fingerprint,
                checked_at,
                success.client,
                success.tools,
            ),
            Ok(Err(error)) => {
                McpRuntimeServerState::unavailable(fingerprint, checked_at, error.to_string())
            }
            Err(_) => McpRuntimeServerState::unavailable(
                fingerprint,
                checked_at,
                format!(
                    "MCP health check timed out after {} seconds",
                    PROBE_TIMEOUT.as_secs()
                ),
            ),
        };
        let mut state = self.state.lock().await;
        let Some(current) = state.servers.get(&server_id) else {
            return;
        };
        if current.fingerprint != fingerprint
            || current.availability_kind != McpAvailabilityKind::Checking
        {
            return;
        }
        state.servers.insert(server_id, next);
        drop(state);
        self.emit_update();
    }

    pub(crate) async fn mark_unavailable(&self, server_id: &str, error: String) {
        let shutdown_client = {
            let mut state = self.state.lock().await;
            let Some(fingerprint) = state
                .servers
                .get(server_id)
                .map(|current| current.fingerprint)
            else {
                return;
            };
            state
                .servers
                .insert(
                    server_id.to_string(),
                    McpRuntimeServerState::unavailable(fingerprint, unix_seconds(), error),
                )
                .and_then(|mut state| state.client.take())
        };
        if let Some(client) = shutdown_client {
            client.shutdown().await;
        }
        self.emit_update();
    }

    pub async fn shutdown(&self) {
        let clients = {
            let mut state = self.state.lock().await;
            let clients = state
                .servers
                .values_mut()
                .filter_map(|server| server.client.take())
                .collect::<Vec<_>>();
            state.servers.clear();
            clients
        };
        for client in clients {
            client.shutdown().await;
        }
        self.emit_update();
    }

    fn emit_update(&self) {
        let _ = self.updates.send(());
    }
}

#[derive(Debug, Default)]
pub(super) struct McpRuntimeRegistryState {
    pub(super) servers: BTreeMap<String, McpRuntimeServerState>,
}

#[derive(Debug, Clone)]
pub(super) struct McpRuntimeServerState {
    fingerprint: u64,
    availability_kind: McpAvailabilityKind,
    availability_message: Option<String>,
    last_checked_at: Option<i64>,
    client: Option<Arc<dyn McpClient>>,
    tools: Vec<McpToolDefinition>,
}

impl McpRuntimeServerState {
    pub(super) fn checking(fingerprint: u64) -> Self {
        Self {
            fingerprint,
            availability_kind: McpAvailabilityKind::Checking,
            availability_message: Some("MCP health check is running".to_string()),
            last_checked_at: None,
            client: None,
            tools: Vec::new(),
        }
    }

    pub(super) fn available(
        fingerprint: u64,
        checked_at: i64,
        client: Arc<dyn McpClient>,
        tools: Vec<McpToolDefinition>,
    ) -> Self {
        let tool_count = tools.len();
        Self {
            fingerprint,
            availability_kind: McpAvailabilityKind::Available,
            availability_message: Some(format!("Available with {tool_count} tools")),
            last_checked_at: Some(checked_at),
            client: Some(client),
            tools,
        }
    }

    fn unavailable(fingerprint: u64, checked_at: i64, error: String) -> Self {
        Self {
            fingerprint,
            availability_kind: McpAvailabilityKind::Unavailable,
            availability_message: Some(error),
            last_checked_at: Some(checked_at),
            client: None,
            tools: Vec::new(),
        }
    }

    pub(super) fn disabled(fingerprint: u64) -> Self {
        Self {
            fingerprint,
            availability_kind: McpAvailabilityKind::Disabled,
            availability_message: Some("MCP server is disabled in configuration".to_string()),
            last_checked_at: None,
            client: None,
            tools: Vec::new(),
        }
    }

    fn missing_credential(fingerprint: u64, message: Option<String>) -> Self {
        Self {
            fingerprint,
            availability_kind: McpAvailabilityKind::MissingCredential,
            availability_message: message,
            last_checked_at: None,
            client: None,
            tools: Vec::new(),
        }
    }
}

#[derive(Clone, Copy)]
enum ProbePolicy {
    ChangedOrUnavailable,
    Force,
}

struct McpAvailableServer {
    server_id: String,
    client: Arc<dyn McpClient>,
    tools: Vec<McpToolDefinition>,
}

fn should_probe_server(
    current: Option<&McpRuntimeServerState>,
    fingerprint: u64,
    policy: ProbePolicy,
) -> bool {
    let Some(current) = current else {
        return true;
    };
    if current.fingerprint != fingerprint {
        return true;
    }
    if current.availability_kind == McpAvailabilityKind::Checking {
        return false;
    }
    match policy {
        ProbePolicy::Force => true,
        ProbePolicy::ChangedOrUnavailable => {
            current.availability_kind != McpAvailabilityKind::Available
        }
    }
}

fn insert_terminal_state(
    state: &mut McpRuntimeRegistryState,
    server_id: String,
    next: McpRuntimeServerState,
    shutdown_clients: &mut Vec<Arc<dyn McpClient>>,
) -> bool {
    let changed = state.servers.get(&server_id).is_none_or(|current| {
        current.fingerprint != next.fingerprint
            || current.availability_kind != next.availability_kind
            || current.availability_message != next.availability_message
    });
    if let Some(mut previous) = state.servers.insert(server_id, next)
        && let Some(client) = previous.client.take()
    {
        shutdown_clients.push(client);
    }
    changed
}

fn server_fingerprint(server: &EffectiveMcpServerConfig) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    server.config.hash(&mut hasher);
    server.status_kind.as_str().hash(&mut hasher);
    server.source_kind.as_str().hash(&mut hasher);
    server.bearer_token.hash(&mut hasher);
    hasher.finish()
}

fn unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
