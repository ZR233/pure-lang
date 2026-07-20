use std::collections::BTreeMap;
use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;

use pl_protocol::{McpHealthSnapshot, PureError, Result};
use serde_json::Value;
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::TurnEngine;
use crate::config::EffectiveMcpServerConfig;
use crate::tool::{
    McpListResourceTemplatesRequest, McpListResourcesRequest, McpReadResourceRequest,
    McpResourceBackend, McpResourceTool, McpResourceToolKind, Tool,
};
use crate::turn::ToolEffect;

use super::contract::{McpCallRequest, McpRuntimeHost};
use super::health::McpAvailabilitySnapshot;
use super::tool_adapter::McpLeaseToolAdapter;

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
    pub effect: Option<ToolEffect>,
}

/// 泛型 MCP worker 所有者。
///
/// 具体 Host 只存在于后台 worker；产品运行时与工具注册表只保存非泛型 handle。
pub struct McpRuntime<H> {
    handle: McpRuntimeHandle,
    host: PhantomData<fn() -> H>,
}

impl<H> fmt::Debug for McpRuntime<H> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("McpRuntime").finish_non_exhaustive()
    }
}

impl<H> McpRuntime<H>
where
    H: McpRuntimeHost,
{
    /// 启动一个由具体 Host 驱动的 MCP worker。
    pub fn new(host: H) -> Self {
        let (commands, receiver) = mpsc::unbounded_channel();
        let (updates, _) = broadcast::channel(64);
        let handle = McpRuntimeHandle {
            commands: commands.clone(),
            updates: updates.clone(),
        };
        tokio::spawn(worker::run(host, receiver, commands, updates));
        Self {
            handle,
            host: PhantomData,
        }
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
        request: McpCallRequest,
    ) -> Result<Value> {
        let (reply, response) = oneshot::channel();
        self.send(RuntimeCommand::CallTool {
            generation,
            server_id,
            request,
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

    pub(super) fn mark_unavailable(
        &self,
        generation: McpGeneration,
        server_id: String,
        error: String,
    ) {
        let _ = self.send(RuntimeCommand::MarkUnavailable {
            generation,
            server_id,
            error,
        });
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

    pub(super) async fn call_tool(
        &self,
        server_id: String,
        request: McpCallRequest,
    ) -> Result<Value> {
        self.guard
            .handle
            .call_tool(self.generation, server_id, request)
            .await
    }

    pub(super) fn mark_unavailable(&self, server_id: String, error: String) {
        self.guard
            .handle
            .mark_unavailable(self.generation, server_id, error);
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
            let adapter = McpLeaseToolAdapter::new(self.clone(), descriptor);
            if core.has_tool(adapter.name()) {
                return Err(PureError::ConfigError(format!(
                    "mcp tool '{}' conflicts with an existing tool",
                    adapter.name()
                )));
            }
            core.register_tool(adapter);
        }
        Ok(())
    }

    fn install_resource_tools(&self, core: &mut TurnEngine) {
        if !self.server_ids.is_empty() {
            let backend = Arc::new(self.clone());
            for kind in McpResourceToolKind::all() {
                if !core.has_tool(kind.name()) {
                    core.register_tool(McpResourceTool::new(*kind, backend.clone()));
                }
            }
        }
    }
}

impl McpResourceBackend for McpTurnLease {
    type Error = PureError;

    async fn list_resources(&self, request: McpListResourcesRequest) -> Result<Value> {
        self.guard
            .handle
            .resource_query(
                self.generation,
                request.server,
                ResourceOperation::ListResources {
                    cursor: request.cursor,
                },
            )
            .await
    }

    async fn list_resource_templates(
        &self,
        request: McpListResourceTemplatesRequest,
    ) -> Result<Value> {
        self.guard
            .handle
            .resource_query(
                self.generation,
                request.server,
                ResourceOperation::ListResourceTemplates {
                    cursor: request.cursor,
                },
            )
            .await
    }

    async fn read_resource(&self, request: McpReadResourceRequest) -> Result<Value> {
        self.guard
            .handle
            .resource_query(
                self.generation,
                Some(request.server),
                ResourceOperation::ReadResource { uri: request.uri },
            )
            .await
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
        request: McpCallRequest,
        reply: oneshot::Sender<Result<Value>>,
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

fn runtime_stopped(error: oneshot::error::RecvError) -> PureError {
    PureError::ToolExecutionFailed {
        tool: "mcp".to_string(),
        error: format!("MCP runtime response channel closed: {error}"),
    }
}
