use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::pin::Pin;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures::Future;
use pl_protocol::{PureError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{Mutex, broadcast, oneshot};

use crate::config::{
    EffectiveMcpServerConfig, McpServerConfig, McpServerStatusKind, McpServerTransport,
    validate_mcp_identifier,
};
use crate::process::{configure_background_command, terminate_process_tree};
use crate::tool::{OutputTruncation, Tool, ToolContext, ToolInput, ToolOutput};

const MCP_PROTOCOL_VERSION: &str = "2025-11-25";
const MCP_TOOL_PREFIX: &str = "mcp__";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const PROBE_TIMEOUT: Duration = Duration::from_secs(8);

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// MCP server 的 JSON-RPC 请求抽象。
///
/// 具体 transport 实现负责连接、请求/响应匹配和生命周期资源持有；
/// tool 适配器只依赖此 trait 发送 `tools/call`。
trait McpClient: fmt::Debug + Send + Sync {
    fn request<'a>(&'a self, method: &'a str, params: Value) -> BoxFuture<'a, Result<Value>>;
    fn notify<'a>(&'a self, method: &'a str, params: Value) -> BoxFuture<'a, Result<()>>;
    fn shutdown<'a>(&'a self) -> BoxFuture<'a, ()>;
}

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
    state: Arc<Mutex<McpRuntimeRegistryState>>,
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
struct McpRuntimeRegistryState {
    servers: BTreeMap<String, McpRuntimeServerState>,
}

#[derive(Debug, Clone)]
struct McpRuntimeServerState {
    fingerprint: u64,
    availability_kind: McpAvailabilityKind,
    availability_message: Option<String>,
    last_checked_at: Option<i64>,
    client: Option<Arc<dyn McpClient>>,
    tools: Vec<McpToolDefinition>,
}

impl McpRuntimeServerState {
    fn checking(fingerprint: u64) -> Self {
        Self {
            fingerprint,
            availability_kind: McpAvailabilityKind::Checking,
            availability_message: Some("MCP health check is running".to_string()),
            last_checked_at: None,
            client: None,
            tools: Vec::new(),
        }
    }

    fn available(
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

    fn disabled(fingerprint: u64) -> Self {
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

struct McpProbeSuccess {
    client: Arc<dyn McpClient>,
    tools: Vec<McpToolDefinition>,
}

#[derive(Debug, Clone)]
pub(crate) struct McpToolAdapter {
    server_id: String,
    exposed_name: String,
    raw_name: String,
    description: String,
    input_schema: Value,
    client: Arc<dyn McpClient>,
    registry: Option<McpRuntimeRegistry>,
}

impl McpToolAdapter {
    fn new(
        server_id: &str,
        definition: McpToolDefinition,
        client: Arc<dyn McpClient>,
        registry: Option<McpRuntimeRegistry>,
    ) -> Result<Self> {
        let exposed_name = exposed_tool_name(server_id, &definition.name)?;
        Ok(Self {
            server_id: server_id.to_string(),
            exposed_name,
            raw_name: definition.name,
            description: definition.description.unwrap_or_default(),
            input_schema: definition.input_schema,
            client,
            registry,
        })
    }
}

impl Tool for McpToolAdapter {
    fn name(&self) -> &str {
        &self.exposed_name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        self.input_schema.clone()
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        _context: ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolOutput>> + Send + 'a>> {
        Box::pin(async move {
            let params = serde_json::json!({
                "name": self.raw_name,
                "arguments": input.arguments,
            });
            let value = match self.client.request("tools/call", params).await {
                Ok(value) => value,
                Err(error) => {
                    if let Some(registry) = &self.registry {
                        registry
                            .mark_unavailable(&self.server_id, error.to_string())
                            .await;
                    }
                    return Err(error);
                }
            };
            let result: McpCallToolResult = match serde_json::from_value(value) {
                Ok(result) => result,
                Err(error) => {
                    if let Some(registry) = &self.registry {
                        registry
                            .mark_unavailable(&self.server_id, error.to_string())
                            .await;
                    }
                    return Err(error.into());
                }
            };
            if result.is_error {
                return Err(PureError::ToolExecutionFailed {
                    tool: self.exposed_name.clone(),
                    error: format_mcp_content(&result.content),
                });
            }
            Ok(ToolOutput {
                description: format_mcp_content(&result.content),
                truncated: OutputTruncation::empty(),
                output_file: PathBuf::new(),
                exit_code: None,
                timed_out: false,
            })
        })
    }
}

async fn probe_server(
    server_id: &str,
    server: &EffectiveMcpServerConfig,
) -> Result<McpProbeSuccess> {
    let client = connect_server(server_id, server).await?;
    initialize_client(&client).await?;
    let tools = list_tools(&client).await?;
    validate_tool_definitions(server_id, &tools)?;
    Ok(McpProbeSuccess { client, tools })
}

fn validate_tool_definitions(server_id: &str, tools: &[McpToolDefinition]) -> Result<()> {
    let mut exposed_names = BTreeSet::new();
    for definition in tools {
        let exposed_name = exposed_tool_name(server_id, &definition.name)?;
        if !exposed_names.insert(exposed_name.clone()) {
            return Err(PureError::ConfigError(format!(
                "mcp server '{server_id}' exposes duplicate tool '{exposed_name}'"
            )));
        }
    }
    Ok(())
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

pub(crate) fn is_mcp_tool_name(name: &str) -> bool {
    name.starts_with(MCP_TOOL_PREFIX)
}

fn exposed_tool_name(server_id: &str, tool_name: &str) -> Result<String> {
    validate_mcp_identifier(server_id, "MCP server id")?;
    validate_mcp_identifier(tool_name, "MCP tool name")?;
    Ok(format!("{MCP_TOOL_PREFIX}{server_id}__{tool_name}"))
}

async fn connect_server(
    server_id: &str,
    server: &EffectiveMcpServerConfig,
) -> Result<Arc<dyn McpClient>> {
    match server.config.transport {
        McpServerTransport::Stdio => {
            let client = StdioMcpClient::spawn(server_id, &server.config).await?;
            Ok(Arc::new(client))
        }
        McpServerTransport::StreamableHttp => {
            let client =
                HttpMcpClient::new(server_id, &server.config, server.bearer_token.clone())?;
            Ok(Arc::new(client))
        }
    }
}

async fn initialize_client(client: &Arc<dyn McpClient>) -> Result<()> {
    client
        .request(
            "initialize",
            serde_json::json!({
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {
                    "name": "pure-lang",
                    "version": env!("CARGO_PKG_VERSION"),
                }
            }),
        )
        .await?;
    client
        .notify("notifications/initialized", serde_json::json!({}))
        .await
}

async fn list_tools(client: &Arc<dyn McpClient>) -> Result<Vec<McpToolDefinition>> {
    let mut cursor = None;
    let mut tools = Vec::new();
    loop {
        let params = cursor
            .as_ref()
            .map(|cursor| serde_json::json!({ "cursor": cursor }))
            .unwrap_or_else(|| serde_json::json!({}));
        let value = client.request("tools/list", params).await?;
        let result: McpListToolsResult = serde_json::from_value(value)?;
        tools.extend(result.tools);
        cursor = result.next_cursor;
        if cursor.is_none() {
            return Ok(tools);
        }
    }
}

struct StdioMcpClient {
    server_id: String,
    stdin: Mutex<Option<ChildStdin>>,
    child: Mutex<Option<Child>>,
    pending: Arc<Mutex<BTreeMap<u64, oneshot::Sender<Result<Value>>>>>,
    next_id: AtomicU64,
}

impl fmt::Debug for StdioMcpClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StdioMcpClient")
            .field("server_id", &self.server_id)
            .finish_non_exhaustive()
    }
}

impl StdioMcpClient {
    async fn spawn(server_id: &str, server: &McpServerConfig) -> Result<Self> {
        let command = server.command.as_deref().unwrap_or_default();
        let mut process = Command::new(command);
        configure_background_command(&mut process);
        process.args(&server.args);
        process.stdin(Stdio::piped());
        process.stdout(Stdio::piped());
        process.stderr(Stdio::piped());
        if let Some(cwd) = server.cwd.as_deref().filter(|cwd| !cwd.trim().is_empty()) {
            process.current_dir(cwd);
        }
        for (key, value) in &server.env {
            process.env(key, value);
        }
        let mut child = process.spawn().map_err(|error| {
            PureError::ConfigError(format!(
                "mcp server '{server_id}' failed to start command '{command}': {error}"
            ))
        })?;
        let stdin = child.stdin.take().ok_or_else(|| {
            PureError::ConfigError(format!("mcp server '{server_id}' stdin is unavailable"))
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            PureError::ConfigError(format!("mcp server '{server_id}' stdout is unavailable"))
        })?;
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    eprintln!("[pl-core] mcp stderr: {line}");
                }
            });
        }
        let pending = Arc::new(Mutex::new(BTreeMap::new()));
        tokio::spawn(read_stdio_responses(
            server_id.to_string(),
            stdout,
            pending.clone(),
        ));
        Ok(Self {
            server_id: server_id.to_string(),
            stdin: Mutex::new(Some(stdin)),
            child: Mutex::new(Some(child)),
            pending,
            next_id: AtomicU64::new(1),
        })
    }
}

impl McpClient for StdioMcpClient {
    fn request<'a>(&'a self, method: &'a str, params: Value) -> BoxFuture<'a, Result<Value>> {
        Box::pin(async move {
            let id = self.next_id.fetch_add(1, Ordering::Relaxed);
            let (tx, rx) = oneshot::channel();
            self.pending.lock().await.insert(id, tx);
            let request = JsonRpcRequest {
                jsonrpc: "2.0",
                id: Some(id),
                method,
                params,
            };
            let mut stdin_guard = self.stdin.lock().await;
            let Some(stdin) = stdin_guard.as_mut() else {
                self.pending.lock().await.remove(&id);
                return Err(PureError::ToolExecutionFailed {
                    tool: self.server_id.clone(),
                    error: "MCP stdio client is shut down".to_string(),
                });
            };
            if let Err(error) = write_stdio_message(stdin, &request).await {
                self.pending.lock().await.remove(&id);
                return Err(error);
            }
            drop(stdin_guard);
            match tokio::time::timeout(REQUEST_TIMEOUT, rx).await {
                Ok(Ok(result)) => result,
                Ok(Err(_)) => Err(PureError::ToolExecutionFailed {
                    tool: self.server_id.clone(),
                    error: "MCP stdio response channel closed".to_string(),
                }),
                Err(_) => {
                    self.pending.lock().await.remove(&id);
                    Err(PureError::ToolExecutionFailed {
                        tool: self.server_id.clone(),
                        error: "MCP stdio request timed out".to_string(),
                    })
                }
            }
        })
    }

    fn notify<'a>(&'a self, method: &'a str, params: Value) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let request = JsonRpcRequest {
                jsonrpc: "2.0",
                id: None,
                method,
                params,
            };
            let mut stdin_guard = self.stdin.lock().await;
            let Some(stdin) = stdin_guard.as_mut() else {
                return Err(PureError::ToolExecutionFailed {
                    tool: self.server_id.clone(),
                    error: "MCP stdio client is shut down".to_string(),
                });
            };
            write_stdio_message(stdin, &request).await
        })
    }

    fn shutdown<'a>(&'a self) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            self.stdin.lock().await.take();
            {
                let mut pending = self.pending.lock().await;
                let pending = std::mem::take(&mut *pending);
                for (_, sender) in pending {
                    let _ = sender.send(Err(PureError::ToolExecutionFailed {
                        tool: self.server_id.clone(),
                        error: "MCP stdio client shut down".to_string(),
                    }));
                }
            }
            let Some(mut child) = self.child.lock().await.take() else {
                return;
            };
            let pid = child.id();
            if tokio::time::timeout(Duration::from_millis(500), child.wait())
                .await
                .is_err()
            {
                terminate_process_tree(pid).await;
                let _ = child.start_kill();
                let _ = child.wait().await;
            }
        })
    }
}

async fn read_stdio_responses(
    server_id: String,
    stdout: tokio::process::ChildStdout,
    pending: Arc<Mutex<BTreeMap<u64, oneshot::Sender<Result<Value>>>>>,
) {
    let mut lines = BufReader::new(stdout).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let Ok(response) = serde_json::from_str::<JsonRpcResponse>(&line) else {
            eprintln!("[pl-core] mcp server '{server_id}' returned invalid JSON: {line}");
            continue;
        };
        let Some(id) = response.id else {
            continue;
        };
        let result = json_rpc_response_result(response);
        if let Some(sender) = pending.lock().await.remove(&id) {
            let _ = sender.send(result);
        }
    }
}

async fn write_stdio_message<T: Serialize>(stdin: &mut ChildStdin, value: &T) -> Result<()> {
    let message = serde_json::to_string(value)?;
    stdin.write_all(message.as_bytes()).await?;
    stdin.write_all(b"\n").await?;
    stdin.flush().await?;
    Ok(())
}

struct HttpMcpClient {
    server_id: String,
    url: String,
    client: reqwest::Client,
    headers: BTreeMap<String, String>,
    bearer_token: Option<String>,
    session_id: Mutex<Option<String>>,
    next_id: AtomicU64,
}

impl fmt::Debug for HttpMcpClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpMcpClient")
            .field("server_id", &self.server_id)
            .field("url", &self.url)
            .finish_non_exhaustive()
    }
}

impl HttpMcpClient {
    fn new(
        server_id: &str,
        server: &McpServerConfig,
        bearer_token_override: Option<String>,
    ) -> Result<Self> {
        let bearer_token = match bearer_token_override {
            Some(token) => Some(token),
            None => match server.bearer_token_env_var.as_deref() {
                Some(env_var) if !env_var.trim().is_empty() => Some(std::env::var(env_var).map_err(
                    |error| {
                        PureError::ConfigError(format!(
                            "mcp server '{server_id}' bearer token env var '{env_var}' is unavailable: {error}"
                        ))
                    },
                )?),
                Some(_) | None => None,
            },
        };
        Ok(Self {
            server_id: server_id.to_string(),
            url: server.url.clone().unwrap_or_default(),
            client: reqwest::Client::new(),
            headers: server.headers.clone(),
            bearer_token,
            session_id: Mutex::new(None),
            next_id: AtomicU64::new(1),
        })
    }

    async fn send_http_rpc(&self, payload: Value) -> Result<Option<Value>> {
        let mut request = self
            .client
            .post(&self.url)
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream");
        for (key, value) in &self.headers {
            request = request.header(key, value);
        }
        if let Some(token) = &self.bearer_token {
            request = request.bearer_auth(token);
        }
        if let Some(session_id) = self.session_id.lock().await.clone() {
            request = request.header("mcp-session-id", session_id);
        }
        let response = request.json(&payload).send().await.map_err(|error| {
            PureError::HttpError(format!(
                "mcp server '{}' request failed: {error}",
                self.server_id
            ))
        })?;
        if let Some(session_id) = response
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
        {
            *self.session_id.lock().await = Some(session_id.to_string());
        }
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(PureError::HttpError(format!(
                "mcp server '{}' returned HTTP {status}: {text}",
                self.server_id
            )));
        }
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let text = response.text().await.map_err(|error| {
            PureError::HttpError(format!(
                "mcp server '{}' response read failed: {error}",
                self.server_id
            ))
        })?;
        if text.trim().is_empty() {
            return Ok(None);
        }
        if content_type.contains("text/event-stream")
            || text.lines().any(|line| line.starts_with("data:"))
        {
            return Ok(Some(parse_sse_json(&text)?));
        }
        let response = serde_json::from_str::<JsonRpcResponse>(&text)?;
        json_rpc_response_result(response).map(Some)
    }
}

impl McpClient for HttpMcpClient {
    fn request<'a>(&'a self, method: &'a str, params: Value) -> BoxFuture<'a, Result<Value>> {
        Box::pin(async move {
            let id = self.next_id.fetch_add(1, Ordering::Relaxed);
            let payload = serde_json::to_value(JsonRpcRequest {
                jsonrpc: "2.0",
                id: Some(id),
                method,
                params,
            })?;
            match tokio::time::timeout(REQUEST_TIMEOUT, self.send_http_rpc(payload)).await {
                Ok(Ok(Some(value))) => Ok(value),
                Ok(Ok(None)) => Err(PureError::ToolExecutionFailed {
                    tool: self.server_id.clone(),
                    error: "MCP HTTP request returned no JSON-RPC response".to_string(),
                }),
                Ok(Err(error)) => Err(error),
                Err(_) => Err(PureError::ToolExecutionFailed {
                    tool: self.server_id.clone(),
                    error: "MCP HTTP request timed out".to_string(),
                }),
            }
        })
    }

    fn notify<'a>(&'a self, method: &'a str, params: Value) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let payload = serde_json::to_value(JsonRpcRequest {
                jsonrpc: "2.0",
                id: None,
                method,
                params,
            })?;
            match tokio::time::timeout(REQUEST_TIMEOUT, self.send_http_rpc(payload)).await {
                Ok(Ok(_)) => Ok(()),
                Ok(Err(error)) => Err(error),
                Err(_) => Err(PureError::ToolExecutionFailed {
                    tool: self.server_id.clone(),
                    error: "MCP HTTP notification timed out".to_string(),
                }),
            }
        })
    }

    fn shutdown<'a>(&'a self) -> BoxFuture<'a, ()> {
        Box::pin(async {})
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonRpcRequest<'a> {
    jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<u64>,
    method: &'a str,
    params: Value,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    id: Option<u64>,
    result: Option<Value>,
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct McpListToolsResult {
    #[serde(default)]
    tools: Vec<McpToolDefinition>,
    next_cursor: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct McpToolDefinition {
    name: String,
    description: Option<String>,
    #[serde(default = "default_input_schema")]
    input_schema: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct McpCallToolResult {
    #[serde(default)]
    content: Vec<Value>,
    #[serde(default)]
    is_error: bool,
}

fn json_rpc_response_result(response: JsonRpcResponse) -> Result<Value> {
    if let Some(error) = response.error {
        return Err(PureError::ToolExecutionFailed {
            tool: "mcp".to_string(),
            error: format!("JSON-RPC error {}: {}", error.code, error.message),
        });
    }
    response
        .result
        .ok_or_else(|| PureError::ToolExecutionFailed {
            tool: "mcp".to_string(),
            error: "JSON-RPC response missing result".to_string(),
        })
}

fn parse_sse_json(text: &str) -> Result<Value> {
    let data = text
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(str::trim)
        .find(|line| !line.is_empty() && *line != "[DONE]")
        .ok_or_else(|| PureError::HttpError("MCP SSE response did not contain data".to_string()))?;
    let response = serde_json::from_str::<JsonRpcResponse>(data)?;
    json_rpc_response_result(response)
}

fn format_mcp_content(content: &[Value]) -> String {
    if content.is_empty() {
        return String::new();
    }
    let parts = content
        .iter()
        .map(format_mcp_content_part)
        .collect::<Vec<_>>();
    parts.join("\n")
}

fn format_mcp_content_part(content: &Value) -> String {
    let Some(object) = content.as_object() else {
        return compact_json(content);
    };
    match object.get("type").and_then(Value::as_str) {
        Some("text") => object
            .get("text")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| compact_json(content)),
        Some("json") => object
            .get("json")
            .map(compact_json)
            .unwrap_or_else(|| compact_json(content)),
        _ => compact_json(&Value::Object(object.clone())),
    }
}

fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| value.to_string())
}

fn default_input_schema() -> Value {
    let mut map = Map::new();
    map.insert("type".to_string(), Value::String("object".to_string()));
    map.insert("properties".to_string(), Value::Object(Map::new()));
    Value::Object(map)
}

fn unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::config::{PureConfig, effective_mcp_servers};
    use pretty_assertions::assert_eq;

    #[derive(Debug)]
    struct FakeMcpClient {
        fail_requests: bool,
        shutdown_count: Option<Arc<AtomicUsize>>,
    }

    impl McpClient for FakeMcpClient {
        fn request<'a>(&'a self, _method: &'a str, _params: Value) -> BoxFuture<'a, Result<Value>> {
            Box::pin(async move {
                if self.fail_requests {
                    return Err(PureError::ToolExecutionFailed {
                        tool: "mcp".to_string(),
                        error: "transport failed".to_string(),
                    });
                }
                Ok(serde_json::json!({
                    "content": [{"type": "text", "text": "ok"}],
                    "isError": false
                }))
            })
        }

        fn notify<'a>(&'a self, _method: &'a str, _params: Value) -> BoxFuture<'a, Result<()>> {
            Box::pin(async { Ok(()) })
        }

        fn shutdown<'a>(&'a self) -> BoxFuture<'a, ()> {
            Box::pin(async move {
                if let Some(count) = &self.shutdown_count {
                    count.fetch_add(1, Ordering::SeqCst);
                }
            })
        }
    }

    fn fake_client(fail_requests: bool) -> Arc<FakeMcpClient> {
        Arc::new(FakeMcpClient {
            fail_requests,
            shutdown_count: None,
        })
    }

    #[test]
    fn exposed_tool_name_prefixes_server_and_tool() {
        let name = exposed_tool_name("github", "search_issues").unwrap();

        assert_eq!(name, "mcp__github__search_issues");
        assert!(is_mcp_tool_name(&name));
    }

    #[test]
    fn exposed_tool_name_rejects_invalid_raw_tool() {
        let error = exposed_tool_name("github", "bad tool").unwrap_err();

        assert!(error.to_string().contains("MCP tool name"));
    }

    #[test]
    fn format_mcp_content_prefers_text_parts() {
        let content = vec![
            serde_json::json!({"type": "text", "text": "hello"}),
            serde_json::json!({"type": "json", "json": {"ok": true}}),
        ];

        assert_eq!(format_mcp_content(&content), "hello\n{\"ok\":true}");
    }

    #[test]
    fn http_client_uses_bearer_token_override() {
        let server = McpServerConfig {
            transport: McpServerTransport::StreamableHttp,
            url: Some("https://example.com/mcp".to_string()),
            bearer_token_env_var: Some("IGNORED_ENV_VAR".to_string()),
            ..Default::default()
        };

        let client =
            HttpMcpClient::new("zhipu_search", &server, Some("coding-plan-key".to_string()))
                .unwrap();

        assert_eq!(client.bearer_token.as_deref(), Some("coding-plan-key"));
    }

    #[tokio::test]
    async fn registry_marks_disabled_and_missing_credential_without_probe() {
        let mut config = PureConfig::default();
        config.mcp_servers.insert(
            "draft".to_string(),
            McpServerConfig {
                enabled: false,
                ..Default::default()
            },
        );
        let registry = McpRuntimeRegistry::new();

        registry.reconcile(effective_mcp_servers(&config)).await;
        let snapshots = registry.snapshots().await;

        assert_eq!(
            snapshots["draft"].availability_kind,
            McpAvailabilityKind::Disabled
        );
        assert_eq!(
            snapshots["zhipu_search"].availability_kind,
            McpAvailabilityKind::MissingCredential
        );
        assert!(registry.available_server_names().await.is_empty());
    }

    #[tokio::test]
    async fn registry_registers_only_available_tools() {
        let registry = McpRuntimeRegistry::new();
        registry.state.lock().await.servers.insert(
            "github".to_string(),
            McpRuntimeServerState::available(
                1,
                123,
                fake_client(false),
                vec![McpToolDefinition {
                    name: "search_issues".to_string(),
                    description: Some("Search issues".to_string()),
                    input_schema: default_input_schema(),
                }],
            ),
        );
        registry
            .state
            .lock()
            .await
            .servers
            .insert("draft".to_string(), McpRuntimeServerState::disabled(1));
        let mut core = crate::PureCore::default_provider().unwrap();

        registry.register_available_tools(&mut core).await.unwrap();

        assert!(core.has_tool("mcp__github__search_issues"));
        assert!(!core.has_tool("mcp__draft__anything"));
    }

    #[tokio::test]
    async fn registry_shutdown_closes_available_clients() {
        let registry = McpRuntimeRegistry::new();
        let shutdown_count = Arc::new(AtomicUsize::new(0));
        registry.state.lock().await.servers.insert(
            "github".to_string(),
            McpRuntimeServerState::available(
                1,
                123,
                Arc::new(FakeMcpClient {
                    fail_requests: false,
                    shutdown_count: Some(shutdown_count.clone()),
                }),
                Vec::new(),
            ),
        );

        registry.shutdown().await;

        assert_eq!(shutdown_count.load(Ordering::SeqCst), 1);
        assert!(registry.snapshots().await.is_empty());
    }

    #[tokio::test]
    async fn reconcile_disabled_server_closes_previous_client() {
        let registry = McpRuntimeRegistry::new();
        let shutdown_count = Arc::new(AtomicUsize::new(0));
        registry.state.lock().await.servers.insert(
            "github".to_string(),
            McpRuntimeServerState::available(
                1,
                123,
                Arc::new(FakeMcpClient {
                    fail_requests: false,
                    shutdown_count: Some(shutdown_count.clone()),
                }),
                Vec::new(),
            ),
        );
        let mut config = PureConfig::default();
        config.mcp_servers.insert(
            "github".to_string(),
            McpServerConfig {
                enabled: false,
                ..Default::default()
            },
        );

        registry.reconcile(effective_mcp_servers(&config)).await;
        let snapshots = registry.snapshots().await;

        assert_eq!(shutdown_count.load(Ordering::SeqCst), 1);
        assert_eq!(
            snapshots["github"].availability_kind,
            McpAvailabilityKind::Disabled
        );
    }

    #[tokio::test]
    async fn tool_transport_failure_marks_server_unavailable() {
        let registry = McpRuntimeRegistry::new();
        registry.state.lock().await.servers.insert(
            "github".to_string(),
            McpRuntimeServerState::available(1, 123, fake_client(false), Vec::new()),
        );
        let adapter = McpToolAdapter::new(
            "github",
            McpToolDefinition {
                name: "search_issues".to_string(),
                description: Some("Search issues".to_string()),
                input_schema: default_input_schema(),
            },
            fake_client(true),
            Some(registry.clone()),
        )
        .unwrap();
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(1);
        let context = ToolContext {
            event_tx,
            options: crate::turn::TurnOptions::default(),
            workspace_access: crate::tool::WorkspaceAccess::WorkspaceOnly,
            mode: crate::turn::CompileMode::Auto,
            workspace_root: std::env::temp_dir(),
            workspace_instructions: None,
            active_subagent: None,
            agent_control: crate::AgentControl::default(),
            lsp_runtime: None,
            parent_session: Arc::new(crate::CoreSession::new()),
        };

        let error = adapter
            .execute(
                ToolInput {
                    arguments: serde_json::json!({}),
                    session_id: "session".to_string(),
                    tool_id: "tool".to_string(),
                },
                context,
            )
            .await
            .unwrap_err();
        let snapshots = registry.snapshots().await;

        assert!(error.to_string().contains("transport failed"));
        assert_eq!(
            snapshots["github"].availability_kind,
            McpAvailabilityKind::Unavailable
        );
        assert_eq!(
            registry.available_server_names().await,
            Vec::<String>::new()
        );
    }
}
