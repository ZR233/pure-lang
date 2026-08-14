use std::collections::HashMap;
use std::fmt;
use std::process::Stdio;
use std::time::Duration;

use pl_protocol::{PureError, Result};
use reqwest::header::{HeaderName, HeaderValue};
use rmcp::model::*;
use rmcp::service::{Peer, RunningService};
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::{StreamableHttpClientTransport, TokioChildProcess};
use rmcp::{ClientCacheConfig, ClientLifecycleMode, ClientServiceExt, RoleClient};
use tokio::process::Command;
use tokio::sync::RwLock;

use crate::config::{EffectiveMcpServerConfig, McpServerTransport};

mod stderr_capture;
mod stdio_program;

use stderr_capture::StderrCapture;

const CLOSE_TIMEOUT: Duration = Duration::from_secs(5);

#[cfg(test)]
type TestConnections = std::sync::Arc<
    std::sync::Mutex<std::collections::BTreeMap<String, std::collections::VecDeque<ConnectedMcp>>>,
>;

/// 建立 MCP transport 所需的完整、仅进程内可见配置。
///
/// 该值可能包含已解析凭证，禁止序列化、trace 或日志输出。
#[derive(Clone)]
pub struct McpConnectRequest {
    pub server_id: String,
    pub server: EffectiveMcpServerConfig,
}

/// 唯一的 MCP 连接入口。
///
/// Connector 只把 PL 的有效配置投影为 rmcp transport 并启动 client service；
/// reconcile、generation、健康和权限不属于该边界。
#[derive(Debug, Clone, Default)]
pub struct McpConnector {
    #[cfg(test)]
    test_connections: Option<TestConnections>,
}

impl McpConnector {
    /// 连接并完成 MCP discovery，或对明确拒绝 discovery 的旧服务回退 initialize。
    pub async fn connect(&self, request: McpConnectRequest) -> Result<ConnectedMcp> {
        #[cfg(test)]
        if let Some(connections) = &self.test_connections {
            return connections
                .lock()
                .expect("MCP test connections lock")
                .get_mut(&request.server_id)
                .and_then(std::collections::VecDeque::pop_front)
                .ok_or_else(|| {
                    connection_config_error(&request.server_id, "no queued MCP test connection")
                });
        }
        match request.server.config.transport {
            McpServerTransport::Stdio => connect_stdio(request).await,
            McpServerTransport::StreamableHttp => connect_http(request).await,
        }
    }

    #[cfg(test)]
    pub(super) fn testing(connections: impl IntoIterator<Item = (String, ConnectedMcp)>) -> Self {
        let mut by_server =
            std::collections::BTreeMap::<String, std::collections::VecDeque<ConnectedMcp>>::new();
        for (server_id, connection) in connections {
            by_server
                .entry(server_id)
                .or_default()
                .push_back(connection);
        }
        Self {
            test_connections: Some(std::sync::Arc::new(std::sync::Mutex::new(by_server))),
        }
    }
}

/// 一个已启动的 rmcp client service。
///
/// `Peer` 可并发克隆；`RunningService` 只由本对象持有，并在最后一个 generation
/// lease 释放后通过 [`Self::close`] 显式关闭。
pub struct ConnectedMcp {
    peer: Peer<RoleClient>,
    owner: RwLock<Option<RunningService<RoleClient, ClientInfo>>>,
}

impl fmt::Debug for ConnectedMcp {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConnectedMcp")
            .field("peer", &self.peer)
            .finish_non_exhaustive()
    }
}

impl ConnectedMcp {
    pub(super) async fn from_running(service: RunningService<RoleClient, ClientInfo>) -> Self {
        let peer = service.peer().clone();
        peer.set_response_cache_config(
            ClientCacheConfig::default().with_serve_stale_on_error(false),
        )
        .await;
        Self {
            peer,
            owner: RwLock::new(Some(service)),
        }
    }

    /// 返回可克隆的 typed rmcp peer。
    pub fn peer(&self) -> Peer<RoleClient> {
        self.peer.clone()
    }

    pub async fn list_tools(&self) -> Result<Vec<Tool>> {
        self.peer
            .list_all_tools()
            .await
            .map_err(|error| connection_error("tools/list", error))
    }

    pub async fn call_tool(
        &self,
        name: String,
        arguments: serde_json::Value,
    ) -> Result<CallToolResult> {
        let mut request = CallToolRequestParams::new(name);
        match arguments {
            serde_json::Value::Object(arguments) => request.arguments = Some(arguments),
            serde_json::Value::Null => {}
            _ => {
                return Err(PureError::ToolExecutionFailed {
                    tool: "mcp".to_string(),
                    error: "MCP tool arguments must be a JSON object".to_string(),
                });
            }
        }
        let owner = self.running_service("tools/call").await?;
        owner
            .as_ref()
            .expect("checked running MCP service")
            .call_tool(request)
            .await
            .map_err(|error| connection_error("tools/call", error))
    }

    pub async fn list_resources(&self, cursor: Option<String>) -> Result<ListResourcesResult> {
        self.peer
            .list_resources(Some(PaginatedRequestParams::default().with_cursor(cursor)))
            .await
            .map_err(|error| connection_error("resources/list", error))
    }

    pub async fn list_resource_templates(
        &self,
        cursor: Option<String>,
    ) -> Result<ListResourceTemplatesResult> {
        self.peer
            .list_resource_templates(Some(PaginatedRequestParams::default().with_cursor(cursor)))
            .await
            .map_err(|error| connection_error("resources/templates/list", error))
    }

    pub async fn read_resource(&self, uri: String) -> Result<ReadResourceResult> {
        let owner = self.running_service("resources/read").await?;
        owner
            .as_ref()
            .expect("checked running MCP service")
            .read_resource(ReadResourceRequestParams::new(uri))
            .await
            .map_err(|error| connection_error("resources/read", error))
    }

    /// 幂等、限时关闭 rmcp service 及其 transport owner。
    pub async fn close(&self) {
        let mut owner = self.owner.write().await;
        let Some(mut service) = owner.take() else {
            return;
        };
        match service.close_with_timeout(CLOSE_TIMEOUT).await {
            Ok(Some(_)) => {}
            Ok(None) => tracing::warn!("MCP service close timed out"),
            Err(error) => tracing::warn!(%error, "MCP service close task failed"),
        }
    }

    async fn running_service(
        &self,
        operation: &str,
    ) -> Result<tokio::sync::RwLockReadGuard<'_, Option<RunningService<RoleClient, ClientInfo>>>>
    {
        let owner = self.owner.read().await;
        if owner.is_none() {
            return Err(PureError::ToolExecutionFailed {
                tool: "mcp".to_string(),
                error: format!("MCP service is closed during {operation}"),
            });
        }
        Ok(owner)
    }
}

async fn connect_stdio(request: McpConnectRequest) -> Result<ConnectedMcp> {
    let config = &request.server.config;
    let command_name = config
        .command
        .as_deref()
        .ok_or_else(|| connection_config_error(&request.server_id, "stdio command is required"))?;
    let program = stdio_program::resolve(command_name)
        .map_err(|error| connection_error(&request.server_id, error))?;
    let mut command = Command::new(program.executable);
    command
        .args(program.prefix_args)
        .args(&config.args)
        .envs(&config.env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(cwd) = config.cwd.as_deref() {
        command.current_dir(cwd);
    }
    let command = crate::process::wrap_background_command(command);

    // `TokioChildProcess::new` 会把 stderr 重置为 inherit；GUI 进程必须像
    // 官方 MCP SDK 一样显式管道化三路 stdio，避免 launcher 重新连接终端。
    let (transport, stderr) = TokioChildProcess::builder(command)
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| connection_error(&request.server_id, error))?;
    let stderr = stderr.map(|stderr| StderrCapture::spawn(stderr, &config.env));
    let service = match client_info()
        .serve_with_lifecycle(transport, lifecycle())
        .await
    {
        Ok(service) => service,
        Err(error) => {
            tokio::task::yield_now().await;
            let error = stderr.as_ref().and_then(StderrCapture::render).map_or_else(
                || error.to_string(),
                |stderr| format!("{error}; stderr: {stderr}"),
            );
            return Err(connection_error(&request.server_id, error));
        }
    };
    Ok(ConnectedMcp::from_running(service).await)
}

async fn connect_http(request: McpConnectRequest) -> Result<ConnectedMcp> {
    let config = &request.server.config;
    let uri = config.url.as_deref().ok_or_else(|| {
        connection_config_error(&request.server_id, "streamable HTTP url is required")
    })?;
    let custom_headers = config
        .headers
        .iter()
        .map(|(name, value)| {
            let name = HeaderName::from_bytes(name.as_bytes())
                .map_err(|error| connection_error(&request.server_id, error))?;
            let value = HeaderValue::from_str(value)
                .map_err(|error| connection_error(&request.server_id, error))?;
            Ok((name, value))
        })
        .collect::<Result<HashMap<_, _>>>()?;
    let mut transport_config = StreamableHttpClientTransportConfig::with_uri(uri.to_string())
        .custom_headers(custom_headers)
        .reinit_on_expired_session(true);
    if let Some(token) = request.server.bearer_token.as_deref() {
        transport_config = transport_config.auth_header(token);
    }
    let transport = StreamableHttpClientTransport::from_config(transport_config);
    let service = client_info()
        .serve_with_lifecycle(transport, lifecycle())
        .await
        .map_err(|error| connection_error(&request.server_id, error))?;
    Ok(ConnectedMcp::from_running(service).await)
}

fn client_info() -> ClientInfo {
    ClientInfo::new(
        ClientCapabilities::default(),
        Implementation::new("pure-lang", env!("CARGO_PKG_VERSION")),
    )
    .with_protocol_version(ProtocolVersion::V_2026_07_28)
}

fn lifecycle() -> ClientLifecycleMode {
    ClientLifecycleMode::Auto {
        preferred_versions: vec![
            ProtocolVersion::V_2026_07_28,
            ProtocolVersion::V_2025_11_25,
            ProtocolVersion::V_2025_06_18,
            ProtocolVersion::V_2025_03_26,
            ProtocolVersion::V_2024_11_05,
        ],
        // 不支持 `server/discover` 的标准 MCP 服务仍通过传统 initialize
        // 协商具体版本；只在对端明确返回 METHOD_NOT_FOUND 时走这条路径。
        legacy_version: Some(ProtocolVersion::V_2025_11_25),
    }
}

fn connection_config_error(server_id: &str, error: &str) -> PureError {
    PureError::ToolExecutionFailed {
        tool: server_id.to_string(),
        error: error.to_string(),
    }
}

fn connection_error(server_id: &str, error: impl fmt::Display) -> PureError {
    PureError::ToolExecutionFailed {
        tool: server_id.to_string(),
        error: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_prefers_discovery_and_allows_proven_legacy_fallback() {
        let ClientLifecycleMode::Auto {
            preferred_versions,
            legacy_version,
        } = lifecycle()
        else {
            panic!("MCP lifecycle must negotiate discovery with a legacy fallback");
        };

        assert_eq!(
            preferred_versions,
            vec![
                ProtocolVersion::V_2026_07_28,
                ProtocolVersion::V_2025_11_25,
                ProtocolVersion::V_2025_06_18,
                ProtocolVersion::V_2025_03_26,
                ProtocolVersion::V_2024_11_05,
            ]
        );
        assert_eq!(legacy_version, Some(ProtocolVersion::V_2025_11_25));
    }
}
