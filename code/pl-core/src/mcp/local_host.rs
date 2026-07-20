use std::sync::Arc;

use pl_protocol::{PureError, Result};
use serde_json::{Value, json};
use tokio::process::Child;

use super::client::McpClient;
use super::contract::{
    McpCallRequest, McpConnectRequest, McpRuntimeHost, McpSession, McpToolDefinition,
};
use super::transport::{client_from_stdio_child, connect_server, initialize_client, list_tools};

/// Studio 和其他本地进程宿主使用的 MCP transport Host。
#[derive(Debug, Clone, Default)]
pub struct LocalMcpRuntimeHost;

/// 本地 stdio 或 Streamable HTTP MCP session。
pub struct LocalMcpSession {
    client: Arc<dyn McpClient>,
}

impl LocalMcpSession {
    /// 接管 Host 已创建的 stdio 子进程，并使用 PL 的统一 MCP wire client 完成握手。
    ///
    /// 容器类产品可只负责创建 `docker exec -i` 进程；协议版本、JSON-RPC、工具与资源
    /// 调用以及关闭语义继续由 PL 维护，避免产品侧出现第二套 MCP client。
    pub async fn from_stdio_child(server_id: impl Into<String>, child: Child) -> Result<Self> {
        let server_id = server_id.into();
        let client = client_from_stdio_child(&server_id, child)?;
        initialize_client(&client).await?;
        Ok(Self { client })
    }
}

impl McpRuntimeHost for LocalMcpRuntimeHost {
    type Error = PureError;
    type Session = LocalMcpSession;

    async fn connect(&self, request: McpConnectRequest) -> Result<Self::Session> {
        let client = connect_server(&request.server_id, &request.server).await?;
        initialize_client(&client).await?;
        Ok(LocalMcpSession { client })
    }
}

impl McpSession for LocalMcpSession {
    type Error = PureError;

    async fn list_tools(&self) -> Result<Vec<McpToolDefinition>> {
        Ok(list_tools(&self.client)
            .await?
            .into_iter()
            .map(|tool| McpToolDefinition {
                name: tool.name,
                description: tool.description,
                input_schema: tool.input_schema,
            })
            .collect())
    }

    async fn call_tool(&self, request: McpCallRequest) -> Result<Value> {
        self.client
            .request(
                "tools/call",
                json!({ "name": request.name, "arguments": request.arguments }),
            )
            .await
    }

    async fn list_resources(&self, cursor: Option<String>) -> Result<Value> {
        self.client
            .request("resources/list", cursor_params(cursor))
            .await
    }

    async fn list_resource_templates(&self, cursor: Option<String>) -> Result<Value> {
        self.client
            .request("resources/templates/list", cursor_params(cursor))
            .await
    }

    async fn read_resource(&self, uri: String) -> Result<Value> {
        self.client
            .request("resources/read", json!({ "uri": uri }))
            .await
    }

    async fn shutdown(&self) {
        self.client.shutdown().await;
    }
}

fn cursor_params(cursor: Option<String>) -> Value {
    cursor
        .map(|cursor| json!({ "cursor": cursor }))
        .unwrap_or_else(|| json!({}))
}
