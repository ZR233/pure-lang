use std::error::Error;
use std::future::Future;

use serde_json::Value;

use crate::config::EffectiveMcpServerConfig;

/// 产品 Host 建立 MCP session 时收到的完整运行时请求。
///
/// 该值只在进程内传递，可能包含已解析凭证，禁止序列化、trace 或日志输出。
#[derive(Clone)]
pub struct McpConnectRequest {
    pub server_id: String,
    pub server: EffectiveMcpServerConfig,
}

/// MCP server 返回的原始工具定义。
#[derive(Debug, Clone, PartialEq)]
pub struct McpToolDefinition {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Value,
}

/// Host session 收到的 MCP 工具调用。
#[derive(Debug, Clone, PartialEq)]
pub struct McpCallRequest {
    pub name: String,
    pub arguments: Value,
}

/// MCP runtime 的产品执行环境端口。
///
/// 实现只负责建立具体 transport session；配置 reconcile、generation、命名、健康状态和
/// tool lifecycle 均由 PL 管理。连接实现不得把 request 中的凭证写入日志。
pub trait McpRuntimeHost: Clone + Send + Sync + 'static {
    type Error: Error + Send + Sync + 'static;
    type Session: McpSession<Error = Self::Error>;

    fn connect(
        &self,
        request: McpConnectRequest,
    ) -> impl Future<Output = std::result::Result<Self::Session, Self::Error>> + Send;
}

/// 一个已初始化的 MCP transport session。
///
/// Session 必须支持并发只读调用；`shutdown` 必须幂等，并释放进程、连接和 pending request。
pub trait McpSession: Send + Sync + 'static {
    type Error: Error + Send + Sync + 'static;

    fn list_tools(
        &self,
    ) -> impl Future<Output = std::result::Result<Vec<McpToolDefinition>, Self::Error>> + Send;

    fn call_tool(
        &self,
        request: McpCallRequest,
    ) -> impl Future<Output = std::result::Result<Value, Self::Error>> + Send;

    fn list_resources(
        &self,
        cursor: Option<String>,
    ) -> impl Future<Output = std::result::Result<Value, Self::Error>> + Send;

    fn list_resource_templates(
        &self,
        cursor: Option<String>,
    ) -> impl Future<Output = std::result::Result<Value, Self::Error>> + Send;

    fn read_resource(
        &self,
        uri: String,
    ) -> impl Future<Output = std::result::Result<Value, Self::Error>> + Send;

    fn shutdown(&self) -> impl Future<Output = ()> + Send;
}
