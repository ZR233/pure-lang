use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use pl_protocol::{McpHealthSnapshot, PureError, Result};
use rmcp::model::CallToolResult;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::TurnEngine;
use crate::config::EffectiveMcpServerConfig;
use crate::tool::cache::ToolCachePolicy;
use crate::tool::{RegisteredTool, ToolDisplayMetadata, ToolRuntimeLockPolicy};
use crate::turn::ToolEffect;

use super::connector::McpConnector;
use super::health::McpAvailabilitySnapshot;
use super::output::call_tool_result_to_output;

mod worker;

/// MCP 配置原子生效的 generation 标识。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct McpGeneration(pub u64);

/// 固定 generation 内的模型可见 MCP 工具。
#[derive(Debug, Clone, PartialEq)]
pub struct McpRuntimeToolDescriptor {
    pub server_id: String,
    pub raw_name: String,
    pub exposed_name: String,
    pub description: String,
    pub input_schema: Value,
    pub output_schema: Option<Value>,
    pub annotations: Option<Value>,
    pub icons: Option<Value>,
    pub metadata: Option<Value>,
    pub effect: Option<ToolEffect>,
}

/// MCP worker 所有者。
pub struct McpRuntime {
    handle: McpRuntimeHandle,
}

impl fmt::Debug for McpRuntime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("McpRuntime").finish_non_exhaustive()
    }
}

impl McpRuntime {
    /// 启动一个由薄 connector 驱动的 MCP worker。
    pub fn new(connector: McpConnector) -> Self {
        let (commands, receiver) = mpsc::unbounded_channel();
        let (updates, _) = broadcast::channel(64);
        let handle = McpRuntimeHandle {
            commands: commands.clone(),
            updates: updates.clone(),
        };
        tokio::spawn(worker::run(connector, receiver, commands, updates));
        Self { handle }
    }

    /// 返回不携带泛型 Host 的命令句柄。
    pub fn handle(&self) -> McpRuntimeHandle {
        self.handle.clone()
    }
}

/// 产品 facade 和工具持有的非泛型 MCP 命令句柄。
#[derive(Clone)]
pub struct McpRuntimeHandle {
    commands: mpsc::UnboundedSender<RuntimeCommand>,
    updates: broadcast::Sender<()>,
}

impl fmt::Debug for McpRuntimeHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("McpRuntimeHandle")
            .finish_non_exhaustive()
    }
}

impl McpRuntimeHandle {
    /// 订阅 generation 或 health 变化通知。
    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.updates.subscribe()
    }

    /// 增量应用配置；新 generation 完全探测后才替换当前 generation。
    pub async fn reconcile(
        &self,
        servers: BTreeMap<String, EffectiveMcpServerConfig>,
    ) -> Result<()> {
        self.send_reconcile(servers, ReconcilePolicy::Changed).await
    }

    /// 强制重新连接所有启用的 server。
    pub async fn recheck(&self, servers: BTreeMap<String, EffectiveMcpServerConfig>) -> Result<()> {
        self.send_reconcile(servers, ReconcilePolicy::Force).await
    }

    /// 获取本轮固定 generation 的工具与资源 lease。
    pub async fn acquire_turn_lease(&self) -> Result<McpTurnLease> {
        let (reply, response) = oneshot::channel();
        self.send(RuntimeCommand::AcquireLease { reply })?;
        let snapshot = response.await.map_err(runtime_stopped)??;
        Ok(McpTurnLease {
            generation: snapshot.generation,
            tools: Arc::new(snapshot.tools),
            server_ids: Arc::new(snapshot.server_ids),
            guard: Arc::new(McpLeaseGuard {
                generation: snapshot.generation,
                handle: self.clone(),
            }),
        })
    }

    /// 返回当前 generation 的兼容 health 快照。
    pub async fn snapshots(&self) -> BTreeMap<String, McpAvailabilitySnapshot> {
        let (reply, response) = oneshot::channel();
        if self.send(RuntimeCommand::Snapshots { reply }).is_err() {
            return BTreeMap::new();
        }
        response.await.unwrap_or_default()
    }

    /// 返回公共无密钥 health 快照。
    pub async fn health_snapshot(&self) -> Result<McpHealthSnapshot> {
        let (reply, response) = oneshot::channel();
        self.send(RuntimeCommand::HealthSnapshot { reply })?;
        response.await.map_err(runtime_stopped)
    }

    /// 返回当前可用于新 turn 的 server 名称。
    pub async fn available_server_names(&self) -> Vec<String> {
        let (reply, response) = oneshot::channel();
        if self
            .send(RuntimeCommand::AvailableServerNames { reply })
            .is_err()
        {
            return Vec::new();
        }
        response.await.unwrap_or_default()
    }

    /// 幂等关闭所有 generation 和 transport session。
    pub async fn shutdown(&self) {
        let (reply, response) = oneshot::channel();
        if self.send(RuntimeCommand::Shutdown { reply }).is_ok() {
            let _ = response.await;
        }
    }

    async fn send_reconcile(
        &self,
        servers: BTreeMap<String, EffectiveMcpServerConfig>,
        policy: ReconcilePolicy,
    ) -> Result<()> {
        let (reply, response) = oneshot::channel();
        self.send(RuntimeCommand::Reconcile {
            servers,
            policy,
            reply,
        })?;
        response.await.map_err(runtime_stopped)?
    }

    pub(super) async fn call_tool(
        &self,
        generation: McpGeneration,
        server_id: String,
        raw_name: String,
        arguments: Value,
    ) -> Result<CallToolResult> {
        let (reply, response) = oneshot::channel();
        self.send(RuntimeCommand::CallTool {
            generation,
            server_id,
            raw_name,
            arguments,
            reply,
        })?;
        response.await.map_err(runtime_stopped)?
    }

    pub(super) async fn resource_query(
        &self,
        generation: McpGeneration,
        server_id: Option<String>,
        operation: ResourceOperation,
    ) -> Result<Value> {
        let (reply, response) = oneshot::channel();
        self.send(RuntimeCommand::ResourceQuery {
            generation,
            server_id,
            operation,
            reply,
        })?;
        response.await.map_err(runtime_stopped)?
    }

    fn send(&self, command: RuntimeCommand) -> Result<()> {
        self.commands
            .send(command)
            .map_err(|_| PureError::ToolExecutionFailed {
                tool: "mcp".to_string(),
                error: "MCP runtime is stopped".to_string(),
            })
    }
}

/// 固定 generation 的 MCP 工具、资源和调用入口。
#[derive(Debug, Clone)]
pub struct McpTurnLease {
    generation: McpGeneration,
    tools: Arc<Vec<McpRuntimeToolDescriptor>>,
    server_ids: Arc<Vec<String>>,
    guard: Arc<McpLeaseGuard>,
}

impl McpTurnLease {
    pub fn generation(&self) -> McpGeneration {
        self.generation
    }

    pub fn tools(&self) -> &[McpRuntimeToolDescriptor] {
        self.tools.as_slice()
    }

    /// 返回本 generation 已就绪并对本轮可见的 MCP server ID。
    pub fn server_ids(&self) -> &[String] {
        self.server_ids.as_slice()
    }

    pub(super) async fn call_tool(
        &self,
        server_id: String,
        raw_name: String,
        arguments: Value,
    ) -> Result<CallToolResult> {
        self.guard
            .handle
            .call_tool(self.generation, server_id, raw_name, arguments)
            .await
    }

    /// 在当前 TurnEngine 原子安装该 lease 的工具和 resource 入口。
    pub fn install(&self, core: &mut TurnEngine) -> Result<()> {
        if !core.mcp_tools_enabled() {
            return Ok(());
        }
        self.install_tools(core)?;
        self.install_resource_tools(core);
        Ok(())
    }

    /// 仅安装本 generation 的 MCP 调用工具。
    ///
    /// 产品需要把 MCP resource 与自身虚拟资源组合时，应使用本方法，并为共享 resource
    /// schema 安装组合 backend。
    pub fn install_tools(&self, core: &mut TurnEngine) -> Result<()> {
        if !core.mcp_tools_enabled() {
            return Ok(());
        }
        for descriptor in self.tools.iter().cloned() {
            let exposed_name = descriptor.exposed_name.clone();
            if core.has_tool(&exposed_name) {
                return Err(PureError::ConfigError(format!(
                    "mcp tool '{}' conflicts with an existing tool",
                    exposed_name
                )));
            }
            core.register_tool(self.registered_tool(descriptor));
        }
        Ok(())
    }

    fn install_resource_tools(&self, core: &mut TurnEngine) {
        if !self.server_ids.is_empty() {
            for kind in ResourceToolKind::all() {
                if !core.has_tool(kind.name()) {
                    core.register_tool(self.registered_resource_tool(*kind));
                }
            }
        }
    }

    fn registered_tool(&self, descriptor: McpRuntimeToolDescriptor) -> RegisteredTool {
        let lease = self.clone();
        let server_id = descriptor.server_id.clone();
        let raw_name = descriptor.raw_name.clone();
        let display_metadata = ToolDisplayMetadata {
            annotations: descriptor.annotations,
            icons: descriptor.icons,
            metadata: descriptor.metadata,
        };
        let handler_server_id = server_id.clone();
        let handler_raw_name = raw_name.clone();
        let mut tool = RegisteredTool::new(
            descriptor.exposed_name,
            descriptor.description,
            descriptor.input_schema,
            move |input, _context| {
                let lease = lease.clone();
                let server_id = handler_server_id.clone();
                let raw_name = handler_raw_name.clone();
                async move {
                    let result = lease
                        .call_tool(server_id.clone(), raw_name.clone(), input.arguments)
                        .await?;
                    call_tool_result_to_output(&server_id, &raw_name, result)
                }
            },
        )
        .with_output_schema(descriptor.output_schema)
        .with_display_metadata(display_metadata)
        .with_cache_policy(ToolCachePolicy::Never)
        .with_runtime_lock_policy(ToolRuntimeLockPolicy::Exclusive);
        if let Some(effect) = descriptor.effect {
            tool = tool.with_effect(effect);
            if effect == ToolEffect::Read {
                tool = tool
                    .with_parallel_tool_calls()
                    .with_runtime_lock_policy(ToolRuntimeLockPolicy::Shared);
            }
        }
        tool
    }

    fn registered_resource_tool(&self, kind: ResourceToolKind) -> RegisteredTool {
        let lease = self.clone();
        RegisteredTool::new(
            kind.name(),
            kind.description(),
            kind.input_schema(),
            move |input, _context| {
                let lease = lease.clone();
                async move {
                    let (server_id, operation) = kind.parse(input.arguments)?;
                    let value = lease
                        .guard
                        .handle
                        .resource_query(lease.generation, server_id, operation)
                        .await?;
                    crate::tool::ToolOutput::json(value)
                }
            },
        )
        .with_effect(ToolEffect::Read)
        .with_parallel_tool_calls()
        .with_runtime_lock_policy(ToolRuntimeLockPolicy::Shared)
        .with_cache_policy(ToolCachePolicy::Never)
    }
}

#[derive(Debug)]
struct McpLeaseGuard {
    generation: McpGeneration,
    handle: McpRuntimeHandle,
}

impl Drop for McpLeaseGuard {
    fn drop(&mut self) {
        let _ = self.handle.commands.send(RuntimeCommand::ReleaseLease {
            generation: self.generation,
        });
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum ReconcilePolicy {
    Changed,
    Force,
}

#[derive(Debug, Clone)]
pub(super) enum ResourceOperation {
    ListResources { cursor: Option<String> },
    ListResourceTemplates { cursor: Option<String> },
    ReadResource { uri: String },
}

pub(super) struct LeaseSnapshot {
    generation: McpGeneration,
    tools: Vec<McpRuntimeToolDescriptor>,
    server_ids: Vec<String>,
}

pub(super) enum RuntimeCommand {
    Reconcile {
        servers: BTreeMap<String, EffectiveMcpServerConfig>,
        policy: ReconcilePolicy,
        reply: oneshot::Sender<Result<()>>,
    },
    AcquireLease {
        reply: oneshot::Sender<Result<LeaseSnapshot>>,
    },
    /// 立即获取当前 generation 的 lease，不等待进行中的 preparation。
    ///
    /// 由 `AcquireLease` 的有界等待任务在超时后发送，避免二次等待形成循环。
    AcquireLeaseImmediate {
        reply: oneshot::Sender<Result<LeaseSnapshot>>,
    },
    ReleaseLease {
        generation: McpGeneration,
    },
    Snapshots {
        reply: oneshot::Sender<BTreeMap<String, McpAvailabilitySnapshot>>,
    },
    HealthSnapshot {
        reply: oneshot::Sender<McpHealthSnapshot>,
    },
    AvailableServerNames {
        reply: oneshot::Sender<Vec<String>>,
    },
    CallTool {
        generation: McpGeneration,
        server_id: String,
        raw_name: String,
        arguments: Value,
        reply: oneshot::Sender<Result<CallToolResult>>,
    },
    ResourceQuery {
        generation: McpGeneration,
        server_id: Option<String>,
        operation: ResourceOperation,
        reply: oneshot::Sender<Result<Value>>,
    },
    MarkUnavailable {
        generation: McpGeneration,
        server_id: String,
        error: String,
    },
    Shutdown {
        reply: oneshot::Sender<()>,
    },
}

const TOOL_LIST_MCP_RESOURCES: &str = "list_mcp_resources";
const TOOL_LIST_MCP_RESOURCE_TEMPLATES: &str = "list_mcp_resource_templates";
const TOOL_READ_MCP_RESOURCE: &str = "read_mcp_resource";

#[derive(Debug, Clone, Copy)]
enum ResourceToolKind {
    ListResources,
    ListResourceTemplates,
    ReadResource,
}

impl ResourceToolKind {
    fn all() -> &'static [Self] {
        &[
            Self::ListResources,
            Self::ListResourceTemplates,
            Self::ReadResource,
        ]
    }

    fn name(self) -> &'static str {
        match self {
            Self::ListResources => TOOL_LIST_MCP_RESOURCES,
            Self::ListResourceTemplates => TOOL_LIST_MCP_RESOURCE_TEMPLATES,
            Self::ReadResource => TOOL_READ_MCP_RESOURCE,
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::ListResources => "List resources provided by MCP servers.",
            Self::ListResourceTemplates => "List resource templates provided by MCP servers.",
            Self::ReadResource => "Read a specific resource from an MCP server.",
        }
    }

    fn input_schema(self) -> Value {
        match self {
            Self::ListResources | Self::ListResourceTemplates => serde_json::json!({
                "type": "object",
                "properties": {
                    "server": { "type": "string" },
                    "cursor": { "type": "string" }
                },
                "additionalProperties": false
            }),
            Self::ReadResource => serde_json::json!({
                "type": "object",
                "properties": {
                    "server": { "type": "string" },
                    "uri": { "type": "string" }
                },
                "required": ["server", "uri"],
                "additionalProperties": false
            }),
        }
    }

    fn parse(self, arguments: Value) -> Result<(Option<String>, ResourceOperation)> {
        match self {
            Self::ListResources => {
                let arguments: ListResourceArguments = parse_resource_arguments(self, arguments)?;
                Ok((
                    arguments.server,
                    ResourceOperation::ListResources {
                        cursor: arguments.cursor,
                    },
                ))
            }
            Self::ListResourceTemplates => {
                let arguments: ListResourceArguments = parse_resource_arguments(self, arguments)?;
                Ok((
                    arguments.server,
                    ResourceOperation::ListResourceTemplates {
                        cursor: arguments.cursor,
                    },
                ))
            }
            Self::ReadResource => {
                let arguments: ReadResourceArguments = parse_resource_arguments(self, arguments)?;
                Ok((
                    Some(arguments.server),
                    ResourceOperation::ReadResource { uri: arguments.uri },
                ))
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListResourceArguments {
    server: Option<String>,
    cursor: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReadResourceArguments {
    server: String,
    uri: String,
}

fn parse_resource_arguments<T: for<'de> Deserialize<'de>>(
    kind: ResourceToolKind,
    arguments: Value,
) -> Result<T> {
    serde_json::from_value(arguments).map_err(|error| PureError::ToolExecutionFailed {
        tool: kind.name().to_string(),
        error: format!("invalid input: {error}"),
    })
}

fn runtime_stopped(error: oneshot::error::RecvError) -> PureError {
    PureError::ToolExecutionFailed {
        tool: "mcp".to_string(),
        error: format!("MCP runtime response channel closed: {error}"),
    }
}
