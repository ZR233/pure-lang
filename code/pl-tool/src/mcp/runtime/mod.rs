use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use pl_protocol::{McpHealthSnapshot, PureError, Result};
use rmcp::model::CallToolResult;
use serde_json::Value;
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::approval::ToolEffect;
use crate::mcp::config::EffectiveMcpServerConfig;

use super::connector::McpConnector;
use super::health::McpAvailabilitySnapshot;

mod worker;

/// MCP 配置原子生效的 generation 标识。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct McpGeneration(pub u64);

/// 远端 annotations 解析出的 typed 安全提示。
///
/// 解析失败时全字段为 `None`（保守默认）；hints 只参与 effect 推导，icons 与
/// 其余 annotations 仅作为展示与审计信息。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolSafetyHints {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_only_hint: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destructive_hint: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_world_hint: Option<bool>,
}

impl McpToolSafetyHints {
    /// 宽容解析 annotations；失败时返回全 `None`。
    pub fn parse(annotations: Option<&Value>) -> Self {
        annotations
            .and_then(|value| serde_json::from_value::<Self>(value.clone()).ok())
            .unwrap_or_default()
    }
}

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
    pub safety_hints: McpToolSafetyHints,
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
    ///
    /// 模型可见工具由 agent 的 step 刷新窗口从固定 generation lease 获取，
    /// 再原子安装到该 agent 自己的工具集。
    pub fn new(connector: McpConnector) -> Self {
        let (commands, receiver) = mpsc::unbounded_channel();
        let (updates, _) = broadcast::channel(64);
        let notification_commands = commands.clone();
        let connector = connector.with_tool_list_changed(move |server_id| {
            let _ = notification_commands.send(RuntimeCommand::ToolListChanged { server_id });
        });
        let handle = McpRuntimeHandle {
            authorization: pl_core::tool::opaque::ToolAuthorization::unique(),
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
    authorization: pl_core::tool::opaque::ToolAuthorization,
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
        let (reply, response) = oneshot::channel();
        self.send(RuntimeCommand::Reconcile { servers, reply })?;
        response.await.map_err(runtime_stopped)?
    }

    /// 显式重置目标连接；范围外 server 继续复用。
    pub async fn reset(
        &self,
        scope: McpResetScope,
        servers: BTreeMap<String, EffectiveMcpServerConfig>,
    ) -> Result<()> {
        let (reply, response) = oneshot::channel();
        self.send(RuntimeCommand::Reset {
            servers,
            scope,
            reply,
        })?;
        response.await.map_err(runtime_stopped)?
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
            resource_server_ids: Arc::new(snapshot.resource_server_ids),
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
    resource_server_ids: Arc<Vec<String>>,
    guard: Arc<McpLeaseGuard>,
}

impl McpTurnLease {
    /// Runtime-local generation authority, independent from stable model declarations.
    pub fn authorization(&self) -> pl_core::tool::opaque::ToolAuthorization {
        self.guard.handle.authorization.scoped(self.generation.0)
    }

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

    /// Reports whether this lease includes a resource-capable service.
    pub fn has_resources(&self) -> bool {
        !self.resource_server_ids.is_empty()
    }

    pub(super) async fn resource_query(
        &self,
        server_id: Option<String>,
        operation: ResourceOperation,
    ) -> Result<Value> {
        if let Some(server) = server_id {
            if !self.resource_server_ids.contains(&server) {
                return Err(PureError::ToolExecutionFailed {
                    tool: "mcp_resource".into(),
                    error: "server has no resource capability in this lease".into(),
                });
            }
            return self
                .guard
                .handle
                .resource_query(self.generation, Some(server), operation)
                .await;
        }
        if let [server] = self.resource_server_ids.as_slice() {
            return self
                .guard
                .handle
                .resource_query(self.generation, Some(server.clone()), operation)
                .await;
        }
        let mut values = serde_json::Map::new();
        for server in self.resource_server_ids.iter() {
            let value = self
                .guard
                .handle
                .resource_query(self.generation, Some(server.clone()), operation.clone())
                .await?;
            values.insert(server.clone(), value);
        }
        Ok(Value::Object(values))
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpResetScope {
    Server { server_id: String },
    All,
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
    resource_server_ids: Vec<String>,
}

pub(super) enum RuntimeCommand {
    Reconcile {
        servers: BTreeMap<String, EffectiveMcpServerConfig>,
        reply: oneshot::Sender<Result<()>>,
    },
    Reset {
        servers: BTreeMap<String, EffectiveMcpServerConfig>,
        scope: McpResetScope,
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
    ToolListChanged {
        server_id: String,
    },
    Shutdown {
        reply: oneshot::Sender<()>,
    },
}

fn runtime_stopped(error: oneshot::error::RecvError) -> PureError {
    PureError::ToolExecutionFailed {
        tool: "mcp".to_string(),
        error: format!("MCP runtime response channel closed: {error}"),
    }
}
