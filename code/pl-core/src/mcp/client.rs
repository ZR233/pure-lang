use std::fmt;
use std::future::Future;
use std::pin::Pin;

use pl_protocol::Result;
use serde_json::Value;

pub(super) type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// MCP server 的 JSON-RPC 请求抽象。
///
/// 具体 transport 实现负责连接、请求/响应匹配和生命周期资源持有；
/// tool 适配器只依赖此 trait 发送 `tools/call`。
pub(super) trait McpClient: fmt::Debug + Send + Sync {
    fn request<'a>(&'a self, method: &'a str, params: Value) -> BoxFuture<'a, Result<Value>>;
    fn notify<'a>(&'a self, method: &'a str, params: Value) -> BoxFuture<'a, Result<()>>;
    fn shutdown<'a>(&'a self) -> BoxFuture<'a, ()>;
}
