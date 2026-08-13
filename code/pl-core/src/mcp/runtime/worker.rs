use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use pl_protocol::{
    McpAvailabilityDescriptor, McpHealthSnapshot, McpServerDescriptor, PureError, Result,
};
use rmcp::model::{CallToolResult, Tool};
use serde_json::{Map, Value};
use tokio::sync::{Notify, broadcast, mpsc};

use crate::config::{EffectiveMcpServerConfig, McpServerConfig, McpServerSourceKind};
use crate::turn::ToolEffect;

use super::{
    LeaseSnapshot, McpGeneration, McpRuntimeToolDescriptor, ReconcilePolicy, ResourceOperation,
    RuntimeCommand,
};
use crate::mcp::health::{McpAvailabilityKind, McpAvailabilitySnapshot};
use crate::mcp::naming::assign_exposed_tool_names;
use crate::mcp::{ConnectedMcp, McpConnector};
const PROBE_TIMEOUT: Duration = Duration::from_secs(8);

const DEFAULT_TOOL_TIMEOUT: Duration = Duration::from_secs(120);

/// `AcquireLease` 等待进行中 MCP preparation 完成的有界上限。
///
/// 启动路径的 MCP reconcile 已在后台执行，turn 开始时若探测尚未完成，
/// lease 请求最多等待该时长以拿到包含新工具的 generation；超时后回落
/// 当前 generation，避免首个 turn 被慢 server 无限阻塞。
const LEASE_ACQUIRE_WAIT: Duration = Duration::from_secs(20);

type ResourceSession = (String, Arc<ConnectedMcp>, Duration, McpErrorRedactor);

mod reconcile;
mod redaction;

use reconcile::{
    ActivePreparation, PendingReconcile, await_preparation, prepare_generation, reject_reconciles,
};
use redaction::McpErrorRedactor;

pub(super) async fn run(
    connector: McpConnector,
    receiver: mpsc::UnboundedReceiver<RuntimeCommand>,
    commands: mpsc::UnboundedSender<RuntimeCommand>,
    updates: broadcast::Sender<()>,
) {
    RuntimeWorker::new(connector, receiver, commands, updates)
        .run()
        .await;
}

struct RuntimeWorker {
    connector: McpConnector,
    receiver: mpsc::UnboundedReceiver<RuntimeCommand>,
    commands: mpsc::UnboundedSender<RuntimeCommand>,
    updates: broadcast::Sender<()>,
    generations: BTreeMap<McpGeneration, RuntimeGeneration>,
    current: McpGeneration,
    next_generation: u64,
    /// preparation 完成信号，用于 `AcquireLease` 的有界等待。
    preparation_notify: Arc<Notify>,
}

impl RuntimeWorker {
    fn new(
        connector: McpConnector,
        receiver: mpsc::UnboundedReceiver<RuntimeCommand>,
        commands: mpsc::UnboundedSender<RuntimeCommand>,
        updates: broadcast::Sender<()>,
    ) -> Self {
        let current = McpGeneration(0);
        Self {
            connector,
            receiver,
            commands,
            updates,
            generations: [(current, RuntimeGeneration::empty(current))]
                .into_iter()
                .collect(),
            current,
            next_generation: 1,
            preparation_notify: Arc::new(Notify::new()),
        }
    }

    async fn run(mut self) {
        let mut pending = VecDeque::new();
        let mut preparation = None;
        loop {
            if preparation.is_none()
                && let Some(request) = pending.pop_front()
            {
                preparation = Some(self.start_preparation(request));
            }
            tokio::select! {
                generation = await_preparation(&mut preparation), if preparation.is_some() => {
                    let active = preparation
                        .take()
                        .expect("completed MCP preparation must exist");
                    self.activate_generation(generation).await;
                    self.preparation_notify.notify_waiters();
                    let _ = active.reply.send(Ok(()));
                }
                command = self.receiver.recv() => {
                    let Some(command) = command else {
                        reject_reconciles(preparation.take(), &mut pending);
                        break;
                    };
                    match command {
                RuntimeCommand::Reconcile {
                    servers,
                    policy,
                    reply,
                } => {
                    pending.push_back(PendingReconcile {
                        servers,
                        policy,
                        reply,
                    });
                }
                RuntimeCommand::AcquireLease { reply } => {
                    if preparation.is_some() || !pending.is_empty() {
                        let notify = self.preparation_notify.clone();
                        let commands = self.commands.clone();
                        tokio::spawn(async move {
                            let _ =
                                tokio::time::timeout(LEASE_ACQUIRE_WAIT, notify.notified()).await;
                            let _ =
                                commands.send(RuntimeCommand::AcquireLeaseImmediate { reply });
                        });
                    } else {
                        let _ = reply.send(self.acquire_lease());
                    }
                }
                RuntimeCommand::AcquireLeaseImmediate { reply } => {
                    let _ = reply.send(self.acquire_lease());
                }
                RuntimeCommand::ReleaseLease { generation } => {
                    self.release_lease(generation).await;
                }
                RuntimeCommand::Snapshots { reply } => {
                    let _ = reply.send(self.snapshots());
                }
                RuntimeCommand::HealthSnapshot { reply } => {
                    let _ = reply.send(self.health_snapshot());
                }
                RuntimeCommand::AvailableServerNames { reply } => {
                    let _ = reply.send(self.available_server_names());
                }
                RuntimeCommand::CallTool {
                    generation,
                    server_id,
                    raw_name,
                    arguments,
                    reply,
                } => self.spawn_tool_call(generation, server_id, raw_name, arguments, reply),
                RuntimeCommand::ResourceQuery {
                    generation,
                    server_id,
                    operation,
                    reply,
                } => self.spawn_resource_query(generation, server_id, operation, reply),
                RuntimeCommand::MarkUnavailable {
                    generation,
                    server_id,
                    error,
                } => self.mark_unavailable(generation, &server_id, error),
                RuntimeCommand::Shutdown { reply } => {
                    reject_reconciles(preparation.take(), &mut pending);
                    self.shutdown().await;
                    let _ = reply.send(());
                    return;
                }
                    }
                }
            }
        }
        self.shutdown().await;
    }

    fn start_preparation(&mut self, request: PendingReconcile) -> ActivePreparation {
        let generation_id = McpGeneration(self.next_generation);
        self.next_generation += 1;
        let reusable = match request.policy {
            ReconcilePolicy::Changed => self.current_generation().servers.clone(),
            ReconcilePolicy::Force => BTreeMap::new(),
        };
        let future = prepare_generation(
            self.connector.clone(),
            generation_id,
            request.servers,
            reusable,
        );
        ActivePreparation {
            future: Box::pin(future),
            reply: request.reply,
        }
    }

    async fn activate_generation(&mut self, mut next: RuntimeGeneration) {
        self.propagate_pending_session_failures(&mut next);
        if let Some(previous) = self.generations.get_mut(&self.current) {
            previous.retired = true;
        }
        self.current = next.id;
        self.generations.insert(next.id, next);
        self.cleanup_retired().await;
        self.emit_update();
    }

    fn propagate_pending_session_failures(&self, next: &mut RuntimeGeneration) {
        for pending in next.servers.values_mut() {
            let Some(session) = pending.session.as_ref() else {
                continue;
            };
            let failed = self.generations.values().find_map(|generation| {
                generation.servers.values().find(|active| {
                    active.availability == McpAvailabilityKind::Unavailable
                        && active
                            .session
                            .as_ref()
                            .is_some_and(|active| Arc::ptr_eq(active, session))
                })
            });
            if let Some(failed) = failed {
                pending.availability = McpAvailabilityKind::Unavailable;
                pending.message = failed.message.clone();
                pending.last_checked_at = failed.last_checked_at;
            }
        }
    }

    fn acquire_lease(&mut self) -> Result<LeaseSnapshot> {
        let generation = self.generations.get_mut(&self.current).ok_or_else(|| {
            PureError::ToolExecutionFailed {
                tool: "mcp".to_string(),
                error: "MCP current generation is unavailable".to_string(),
            }
        })?;
        generation.leases += 1;
        let tools = generation
            .servers
            .values()
            .filter(|server| server.availability == McpAvailabilityKind::Available)
            .flat_map(|server| server.tools.clone())
            .collect();
        let server_ids = generation
            .servers
            .values()
            .filter(|server| server.availability == McpAvailabilityKind::Available)
            .map(|server| server.descriptor.id.clone())
            .collect();
        Ok(LeaseSnapshot {
            generation: generation.id,
            tools,
            server_ids,
        })
    }

    async fn release_lease(&mut self, generation: McpGeneration) {
        if let Some(state) = self.generations.get_mut(&generation) {
            state.leases = state.leases.saturating_sub(1);
        }
        self.cleanup_retired().await;
    }

    fn snapshots(&self) -> BTreeMap<String, McpAvailabilitySnapshot> {
        self.current_generation()
            .servers
            .iter()
            .map(|(server_id, server)| {
                (
                    server_id.clone(),
                    McpAvailabilitySnapshot {
                        server_id: server_id.clone(),
                        availability_kind: server.availability,
                        availability_message: server.message.clone(),
                        last_checked_at: server.last_checked_at,
                        tool_count: (server.availability == McpAvailabilityKind::Available)
                            .then_some(server.tools.len()),
                    },
                )
            })
            .collect()
    }

    fn health_snapshot(&self) -> McpHealthSnapshot {
        let generation = self.current_generation();
        McpHealthSnapshot {
            generation: generation.id.0,
            servers: generation
                .servers
                .values()
                .map(|server| McpAvailabilityDescriptor {
                    server: server.descriptor.clone(),
                    availability: server.availability.as_str().to_string(),
                    message: server.message.clone(),
                    last_checked_at: server.last_checked_at,
                    tool_count: (server.availability == McpAvailabilityKind::Available)
                        .then_some(server.tools.len()),
                })
                .collect(),
        }
    }

    fn available_server_names(&self) -> Vec<String> {
        self.current_generation()
            .servers
            .values()
            .filter(|server| server.availability == McpAvailabilityKind::Available)
            .map(|server| server.descriptor.id.clone())
            .collect()
    }

    fn spawn_tool_call(
        &self,
        generation: McpGeneration,
        server_id: String,
        raw_name: String,
        arguments: Value,
        reply: tokio::sync::oneshot::Sender<Result<CallToolResult>>,
    ) {
        let session = self.session(generation, &server_id);
        let commands = self.commands.clone();
        tokio::spawn(async move {
            let result = match session {
                Ok((session, request_timeout, redactor)) => {
                    tokio::time::timeout(request_timeout, session.call_tool(raw_name, arguments))
                        .await
                        .map_err(|_| {
                            format!(
                                "request timed out after {} seconds",
                                request_timeout.as_secs()
                            )
                        })
                        .and_then(|result| {
                            result.map_err(|error| redactor.redact(error.to_string()))
                        })
                        .map_err(|message| {
                            let _ = commands.send(RuntimeCommand::MarkUnavailable {
                                generation,
                                server_id: server_id.clone(),
                                error: message.clone(),
                            });
                            PureError::ToolExecutionFailed {
                                tool: server_id,
                                error: message,
                            }
                        })
                }
                Err(error) => Err(error),
            };
            let _ = reply.send(result);
        });
    }

    fn spawn_resource_query(
        &self,
        generation: McpGeneration,
        server_id: Option<String>,
        operation: ResourceOperation,
        reply: tokio::sync::oneshot::Sender<Result<Value>>,
    ) {
        let sessions = self.sessions(generation, server_id.as_deref());
        let commands = self.commands.clone();
        tokio::spawn(async move {
            let result = resource_query(sessions, generation, operation, commands).await;
            let _ = reply.send(result);
        });
    }

    fn session(
        &self,
        generation: McpGeneration,
        server_id: &str,
    ) -> Result<(Arc<ConnectedMcp>, Duration, McpErrorRedactor)> {
        self.generations
            .get(&generation)
            .and_then(|generation| generation.servers.get(server_id))
            .and_then(|server| {
                (server.availability == McpAvailabilityKind::Available)
                    .then(|| server.session.clone())
                    .flatten()
                    .map(|session| (session, server.request_timeout, server.redactor.clone()))
            })
            .ok_or_else(|| PureError::ToolExecutionFailed {
                tool: "mcp".to_string(),
                error: format!(
                    "MCP server '{server_id}' is unavailable in generation {}",
                    generation.0
                ),
            })
    }

    fn sessions(
        &self,
        generation: McpGeneration,
        server_id: Option<&str>,
    ) -> Result<Vec<ResourceSession>> {
        let state =
            self.generations
                .get(&generation)
                .ok_or_else(|| PureError::ToolExecutionFailed {
                    tool: "mcp".to_string(),
                    error: format!("MCP generation {} is unavailable", generation.0),
                })?;
        match server_id {
            Some(server_id) => {
                let (session, request_timeout, redactor) = self.session(generation, server_id)?;
                Ok(vec![(
                    server_id.to_string(),
                    session,
                    request_timeout,
                    redactor,
                )])
            }
            None => Ok(state
                .servers
                .values()
                .filter_map(|server| {
                    (server.availability == McpAvailabilityKind::Available)
                        .then(|| server.session.clone())
                        .flatten()
                        .map(|session| {
                            (
                                server.descriptor.id.clone(),
                                session,
                                server.request_timeout,
                                server.redactor.clone(),
                            )
                        })
                })
                .collect()),
        }
    }

    fn mark_unavailable(&mut self, generation: McpGeneration, server_id: &str, error: String) {
        let failed = self
            .generations
            .get(&generation)
            .and_then(|generation| generation.servers.get(server_id))
            .and_then(|server| server.session.clone());
        for generation in self.generations.values_mut() {
            for server in generation.servers.values_mut() {
                let same_session = failed.as_ref().is_some_and(|failed| {
                    server
                        .session
                        .as_ref()
                        .is_some_and(|session| Arc::ptr_eq(failed, session))
                });
                if same_session {
                    server.availability = McpAvailabilityKind::Unavailable;
                    server.message = Some(error.clone());
                    server.last_checked_at = Some(unix_seconds());
                }
            }
        }
        self.emit_update();
    }

    async fn cleanup_retired(&mut self) {
        let retired = self
            .generations
            .iter()
            .filter(|(_, generation)| generation.retired && generation.leases == 0)
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        for id in retired {
            let Some(generation) = self.generations.remove(&id) else {
                continue;
            };
            for session in unique_sessions(generation.servers) {
                if Arc::strong_count(&session) == 1 {
                    session.close().await;
                }
            }
        }
    }

    async fn shutdown(&mut self) {
        let generations = std::mem::take(&mut self.generations);
        let mut sessions = Vec::new();
        let mut seen = BTreeSet::new();
        for generation in generations.into_values() {
            for session in generation
                .servers
                .into_values()
                .filter_map(|server| server.session)
            {
                let identity = Arc::as_ptr(&session) as usize;
                if seen.insert(identity) {
                    sessions.push(session);
                }
            }
        }
        for session in sessions {
            session.close().await;
        }
        self.emit_update();
    }

    fn current_generation(&self) -> &RuntimeGeneration {
        self.generations
            .get(&self.current)
            .expect("MCP current generation must exist")
    }

    fn emit_update(&self) {
        let _ = self.updates.send(());
    }
}

struct RuntimeGeneration {
    id: McpGeneration,
    servers: BTreeMap<String, RuntimeServer>,
    leases: usize,
    retired: bool,
}

impl RuntimeGeneration {
    fn empty(id: McpGeneration) -> Self {
        Self {
            id,
            servers: BTreeMap::new(),
            leases: 0,
            retired: false,
        }
    }
}

struct RuntimeServer {
    descriptor: McpServerDescriptor,
    fingerprint: u64,
    availability: McpAvailabilityKind,
    message: Option<String>,
    last_checked_at: Option<i64>,
    session: Option<Arc<ConnectedMcp>>,
    definitions: Vec<Tool>,
    tools: Vec<McpRuntimeToolDescriptor>,
    request_timeout: Duration,
    tool_effect: Option<ToolEffect>,
    redactor: McpErrorRedactor,
}

impl Clone for RuntimeServer {
    fn clone(&self) -> Self {
        Self {
            descriptor: self.descriptor.clone(),
            fingerprint: self.fingerprint,
            availability: self.availability,
            message: self.message.clone(),
            last_checked_at: self.last_checked_at,
            session: self.session.clone(),
            definitions: self.definitions.clone(),
            tools: self.tools.clone(),
            request_timeout: self.request_timeout,
            tool_effect: self.tool_effect,
            redactor: self.redactor.clone(),
        }
    }
}

impl RuntimeServer {
    fn terminal(
        config: &EffectiveMcpServerConfig,
        fingerprint: u64,
        availability: McpAvailabilityKind,
        message: Option<String>,
    ) -> Self {
        Self {
            descriptor: server_descriptor(config),
            fingerprint,
            availability,
            message,
            last_checked_at: None,
            session: None,
            definitions: Vec::new(),
            tools: Vec::new(),
            request_timeout: configured_tool_timeout(config.config.tool_timeout_secs),
            tool_effect: config.tool_effect,
            redactor: McpErrorRedactor::new(config),
        }
    }

    fn available(
        descriptor: McpServerDescriptor,
        fingerprint: u64,
        session: Arc<ConnectedMcp>,
        definitions: Vec<Tool>,
        request_timeout: Duration,
        tool_effect: Option<ToolEffect>,
        redactor: McpErrorRedactor,
    ) -> Self {
        Self {
            descriptor,
            fingerprint,
            availability: McpAvailabilityKind::Available,
            message: Some(format!("Available with {} tools", definitions.len())),
            last_checked_at: Some(unix_seconds()),
            session: Some(session),
            definitions,
            tools: Vec::new(),
            request_timeout,
            tool_effect,
            redactor,
        }
    }

    fn unavailable(
        descriptor: McpServerDescriptor,
        fingerprint: u64,
        error: String,
        redactor: McpErrorRedactor,
    ) -> Self {
        Self {
            descriptor,
            fingerprint,
            availability: McpAvailabilityKind::Unavailable,
            message: Some(error),
            last_checked_at: Some(unix_seconds()),
            session: None,
            definitions: Vec::new(),
            tools: Vec::new(),
            request_timeout: PROBE_TIMEOUT,
            tool_effect: None,
            redactor,
        }
    }
}

fn assign_tool_descriptors(servers: &mut BTreeMap<String, RuntimeServer>) {
    let names = assign_exposed_tool_names(servers.iter().flat_map(|(server_id, server)| {
        server
            .definitions
            .iter()
            .map(move |definition| (server_id.as_str(), definition.name.as_ref()))
    }));
    let mut names = names.into_iter();
    for server in servers.values_mut() {
        server.tools = server
            .definitions
            .iter()
            .map(|definition| McpRuntimeToolDescriptor {
                server_id: server.descriptor.id.clone(),
                raw_name: definition.name.to_string(),
                exposed_name: names.next().expect("every MCP tool receives a name"),
                description: definition
                    .description
                    .as_deref()
                    .unwrap_or_default()
                    .to_string(),
                input_schema: Value::Object(definition.input_schema.as_ref().clone()),
                output_schema: definition
                    .output_schema
                    .as_ref()
                    .map(|schema| Value::Object(schema.as_ref().clone())),
                annotations: serialize_optional(&definition.annotations),
                icons: serialize_optional(&definition.icons),
                metadata: serialize_optional(&definition.meta),
                effect: server.tool_effect,
            })
            .collect();
    }
}

async fn resource_query(
    sessions: Result<Vec<ResourceSession>>,
    generation: McpGeneration,
    operation: ResourceOperation,
    commands: mpsc::UnboundedSender<RuntimeCommand>,
) -> Result<Value> {
    let sessions = sessions?;
    let explicit = sessions.len() == 1;
    let mut values = Map::new();
    for (server_id, session, request_timeout, redactor) in sessions {
        let result = tokio::time::timeout(request_timeout, async {
            match &operation {
                ResourceOperation::ListResources { cursor } => session
                    .list_resources(cursor.clone())
                    .await
                    .and_then(serialize_resource_result),
                ResourceOperation::ListResourceTemplates { cursor } => session
                    .list_resource_templates(cursor.clone())
                    .await
                    .and_then(serialize_resource_result),
                ResourceOperation::ReadResource { uri } => session
                    .read_resource(uri.clone())
                    .await
                    .and_then(serialize_resource_result),
            }
        })
        .await;
        let value = result
            .map_err(|_| {
                format!(
                    "request timed out after {} seconds",
                    request_timeout.as_secs()
                )
            })
            .and_then(|result| result.map_err(|error| redactor.redact(error.to_string())))
            .map_err(|message| {
                let _ = commands.send(RuntimeCommand::MarkUnavailable {
                    generation,
                    server_id: server_id.clone(),
                    error: message.clone(),
                });
                PureError::ToolExecutionFailed {
                    tool: server_id.clone(),
                    error: message,
                }
            })?;
        if explicit {
            return Ok(value);
        }
        values.insert(server_id, value);
    }
    Ok(Value::Object(values))
}

fn unique_sessions(servers: BTreeMap<String, RuntimeServer>) -> Vec<Arc<ConnectedMcp>> {
    let mut seen = BTreeSet::new();
    servers
        .into_values()
        .filter_map(|server| server.session)
        .filter(|session| seen.insert(Arc::as_ptr(session) as usize))
        .collect()
}

fn server_descriptor(config: &EffectiveMcpServerConfig) -> McpServerDescriptor {
    McpServerDescriptor {
        id: config.id.clone(),
        source: config.source_kind.as_str().to_string(),
        transport: config.config.transport.as_str().to_string(),
        endpoint: config.config.endpoint_summary(),
        built_in: config.source_kind == McpServerSourceKind::BuiltIn,
    }
}

fn server_fingerprint(server: &EffectiveMcpServerConfig) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    server.config.hash(&mut hasher);
    server.status_kind.as_str().hash(&mut hasher);
    server.source_kind.as_str().hash(&mut hasher);
    server.bearer_token.hash(&mut hasher);
    server.tool_effect.hash(&mut hasher);
    hasher.finish()
}

fn configured_startup_timeout(seconds: Option<u64>) -> Duration {
    seconds.map(Duration::from_secs).unwrap_or(PROBE_TIMEOUT)
}

fn configured_tool_timeout(seconds: Option<u64>) -> Duration {
    seconds
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_TOOL_TIMEOUT)
}

fn filter_tool_definitions(tools: Vec<Tool>, config: &McpServerConfig) -> Vec<Tool> {
    let enabled = config
        .enabled_tools
        .as_ref()
        .map(|names| names.iter().map(String::as_str).collect::<BTreeSet<_>>());
    let disabled = config
        .disabled_tools
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    tools
        .into_iter()
        .filter(|tool| {
            let name = tool.name.as_ref();
            enabled.as_ref().is_none_or(|names| names.contains(name)) && !disabled.contains(name)
        })
        .map(normalize_tool_definition)
        .collect()
}

fn normalize_tool_definition(mut tool: Tool) -> Tool {
    let mut schema = tool.input_schema.as_ref().clone();
    schema
        .entry("type".to_string())
        .or_insert_with(|| Value::String("object".to_string()));
    if schema
        .get("properties")
        .is_none_or(serde_json::Value::is_null)
    {
        schema.insert("properties".to_string(), Value::Object(Map::new()));
    }
    tool.input_schema = Arc::new(schema);
    tool
}

fn serialize_optional<T: serde::Serialize>(value: &Option<T>) -> Option<Value> {
    value
        .as_ref()
        .and_then(|value| serde_json::to_value(value).ok())
}

fn serialize_resource_result(result: impl serde::Serialize) -> Result<Value> {
    serde_json::to_value(result).map_err(PureError::from)
}

fn unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
