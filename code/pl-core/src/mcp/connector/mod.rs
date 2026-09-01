use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use pl_protocol::{PureError, Result};
use reqwest::header::{HeaderName, HeaderValue};
use rmcp::handler::client::ClientHandler;
use rmcp::model::*;
use rmcp::service::{ClientInitializeError, NotificationContext, Peer, RunningService};
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::{StreamableHttpClientTransport, TokioChildProcess};
use rmcp::{ClientCacheConfig, ClientLifecycleMode, ClientServiceExt, RoleClient};
use tokio::process::Command;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

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
type ToolListChangedSink = Arc<dyn Fn(String) + Send + Sync>;

#[derive(Clone, Default)]
pub struct McpConnector {
    tool_list_changed: Option<ToolListChangedSink>,
    #[cfg(test)]
    test_connections: Option<TestConnections>,
}

impl fmt::Debug for McpConnector {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("McpConnector")
            .field("tool_list_changed", &self.tool_list_changed.is_some())
            .finish_non_exhaustive()
    }
}

impl McpConnector {
    /// 按 transport 选择兼容的 MCP 启动协商并建立连接。
    pub async fn connect(&self, request: McpConnectRequest) -> Result<ConnectedMcp> {
        #[cfg(test)]
        if let Some(connections) = &self.test_connections {
            let connection = connections
                .lock()
                .expect("MCP test connections lock")
                .get_mut(&request.server_id)
                .and_then(std::collections::VecDeque::pop_front)
                .ok_or_else(|| {
                    connection_config_error(&request.server_id, "no queued MCP test connection")
                })?;
            connection
                .set_tool_list_changed(request.server_id.clone(), self.tool_list_changed.clone())
                .await;
            return Ok(connection);
        }
        match request.server.config.transport {
            McpServerTransport::Stdio => {
                connect_stdio(request, self.tool_list_changed.clone()).await
            }
            McpServerTransport::StreamableHttp => {
                connect_http(request, self.tool_list_changed.clone()).await
            }
        }
    }

    pub(crate) fn with_tool_list_changed(
        mut self,
        handler: impl Fn(String) + Send + Sync + 'static,
    ) -> Self {
        self.tool_list_changed = Some(Arc::new(handler));
        self
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
            tool_list_changed: None,
            test_connections: Some(std::sync::Arc::new(std::sync::Mutex::new(by_server))),
        }
    }
}

#[derive(Clone)]
pub(super) struct McpClientHandler {
    info: ClientInfo,
    server_id: Arc<std::sync::RwLock<String>>,
    tool_list_changed: Arc<std::sync::RwLock<Option<ToolListChangedSink>>>,
}

impl McpClientHandler {
    fn new(
        info: ClientInfo,
        server_id: String,
        tool_list_changed: Option<ToolListChangedSink>,
    ) -> Self {
        Self {
            info,
            server_id: Arc::new(std::sync::RwLock::new(server_id)),
            tool_list_changed: Arc::new(std::sync::RwLock::new(tool_list_changed)),
        }
    }

    #[cfg(test)]
    pub(super) fn without_notifications(info: ClientInfo) -> Self {
        Self::new(info, String::new(), None)
    }
}

impl ClientHandler for McpClientHandler {
    fn get_info(&self) -> ClientInfo {
        self.info.clone()
    }

    fn on_tool_list_changed(
        &self,
        _context: NotificationContext<RoleClient>,
    ) -> impl Future<Output = ()> + Send + '_ {
        let server_id = self
            .server_id
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let handler = self
            .tool_list_changed
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        async move {
            if let Some(handler) = handler {
                handler(server_id);
            }
        }
    }
}

/// 一个已启动的 rmcp client service。
///
/// `Peer` 可并发克隆；`RunningService` 只由本对象持有，并在最后一个 generation
/// lease 释放后通过 [`Self::close`] 显式关闭。
pub struct ConnectedMcp {
    peer: Peer<RoleClient>,
    owner: RwLock<Option<RunningService<RoleClient, McpClientHandler>>>,
    tool_subscription: RwLock<Option<ToolListSubscription>>,
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
    pub(super) async fn from_running(
        service: RunningService<RoleClient, McpClientHandler>,
    ) -> Self {
        let peer = service.peer().clone();
        peer.set_response_cache_config(
            ClientCacheConfig::default().with_serve_stale_on_error(false),
        )
        .await;
        let tool_subscription = start_tool_list_subscription(&service).await;
        Self {
            peer,
            owner: RwLock::new(Some(service)),
            tool_subscription: RwLock::new(tool_subscription),
        }
    }

    #[cfg(test)]
    async fn set_tool_list_changed(&self, server_id: String, handler: Option<ToolListChangedSink>) {
        let owner = self.owner.read().await;
        let Some(service) = owner.as_ref() else {
            return;
        };
        *service
            .service()
            .server_id
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = server_id;
        *service
            .service()
            .tool_list_changed
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = handler;
    }

    #[cfg(test)]
    pub(super) async fn has_tool_subscription(&self) -> bool {
        self.tool_subscription.read().await.is_some()
    }

    /// 返回可克隆的 typed rmcp peer。
    pub fn peer(&self) -> Peer<RoleClient> {
        self.peer.clone()
    }

    /// Whether the server declared the MCP resources capability during discovery.
    pub fn supports_resources(&self) -> bool {
        self.peer
            .peer_info()
            .is_some_and(|info| info.capabilities.resources.is_some())
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
        if let Some(subscription) = self.tool_subscription.write().await.take() {
            subscription.cancel.cancel();
            let _ = tokio::time::timeout(CLOSE_TIMEOUT, subscription.task).await;
        }
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
    ) -> Result<
        tokio::sync::RwLockReadGuard<'_, Option<RunningService<RoleClient, McpClientHandler>>>,
    > {
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

struct ToolListSubscription {
    cancel: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}

async fn start_tool_list_subscription(
    service: &RunningService<RoleClient, McpClientHandler>,
) -> Option<ToolListSubscription> {
    let peer_info = service.peer().peer_info()?;
    let supports_list_changed = peer_info
        .capabilities
        .tools
        .as_ref()
        .and_then(|tools| tools.list_changed)
        == Some(true);
    if peer_info.protocol_version != ProtocolVersion::V_2026_07_28 || !supports_list_changed {
        return None;
    }
    let mut subscription = match service
        .peer()
        .listen(SubscriptionFilter::builder().tools_list_changed().build())
        .await
    {
        Ok(subscription) => subscription,
        Err(error) => {
            tracing::warn!(%error, "failed to subscribe to MCP tools/list_changed");
            return None;
        }
    };
    let server_id = service.service().server_id.clone();
    let handler = service.service().tool_list_changed.clone();
    let cancel = CancellationToken::new();
    let task_cancel = cancel.clone();
    let task = tokio::spawn(async move {
        loop {
            tokio::select! {
                () = task_cancel.cancelled() => {
                    let _ = subscription.cancel().await;
                    break;
                }
                notification = subscription.next() => match notification {
                    Ok(Some(_notification)) => {
                        let callback = handler
                            .read()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .clone();
                        if let Some(callback) = callback {
                            callback(
                                server_id
                                    .read()
                                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                                    .clone(),
                            );
                        }
                    }
                    Ok(None) => break,
                    Err(error) => {
                        tracing::warn!(%error, "MCP tools/list_changed subscription ended");
                        break;
                    }
                }
            }
        }
    });
    Some(ToolListSubscription { cancel, task })
}

async fn connect_stdio(
    request: McpConnectRequest,
    tool_list_changed: Option<ToolListChangedSink>,
) -> Result<ConnectedMcp> {
    let config = &request.server.config;
    let command_name = config
        .command
        .as_deref()
        .ok_or_else(|| connection_config_error(&request.server_id, "stdio command is required"))?;
    let program = stdio_program::resolve(command_name).map_err(|error| {
        connection_error(
            &request.server_id,
            format!("failed to resolve stdio command {command_name}: {error}"),
        )
    })?;
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
        .map_err(|error| {
            connection_error(
                &request.server_id,
                format!("failed to start stdio command {command_name}: {error}"),
            )
        })?;
    let stderr = stderr.map(|stderr| StderrCapture::spawn(stderr, &config.env));
    let service = match client_handler(
        &request.server_id,
        ProtocolVersion::V_2026_07_28,
        tool_list_changed,
    )
    .serve_with_lifecycle(transport, discovery_lifecycle())
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

async fn connect_http(
    request: McpConnectRequest,
    tool_list_changed: Option<ToolListChangedSink>,
) -> Result<ConnectedMcp> {
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
    let transport = StreamableHttpClientTransport::from_config(transport_config.clone());
    let service = match client_handler(
        &request.server_id,
        ProtocolVersion::V_2026_07_28,
        tool_list_changed.clone(),
    )
    .serve_with_lifecycle(transport, discovery_lifecycle())
    .await
    {
        Ok(service) => service,
        Err(discovery_error) if should_retry_http_with_initialize(&discovery_error) => {
            // rmcp 会先耗尽 startup SSE，只转发成功 response；传统服务返回的
            // METHOD_NOT_FOUND error 因而表现为关闭 discover response。失败的 worker
            // 不可复用，必须用一个全新的 transport 走标准 initialize。
            let transport = StreamableHttpClientTransport::from_config(transport_config);
            client_handler(
                &request.server_id,
                ProtocolVersion::V_2025_11_25,
                tool_list_changed,
            )
                .serve_with_lifecycle(transport, ClientLifecycleMode::Initialize)
                .await
                .map_err(|initialize_error| {
                    connection_error(
                        &request.server_id,
                        format!(
                            "discovery failed ({discovery_error}); standard initialize failed ({initialize_error})"
                        ),
                    )
                })?
        }
        Err(error) => return Err(connection_error(&request.server_id, error)),
    };
    Ok(ConnectedMcp::from_running(service).await)
}

fn should_retry_http_with_initialize(error: &ClientInitializeError) -> bool {
    matches!(
        error,
        ClientInitializeError::ConnectionClosed(context) if context == "discover response"
    )
}

fn client_info(protocol_version: ProtocolVersion) -> ClientInfo {
    ClientInfo::new(
        ClientCapabilities::default(),
        Implementation::new("pure-lang", env!("CARGO_PKG_VERSION")),
    )
    .with_protocol_version(protocol_version)
}

fn client_handler(
    server_id: &str,
    protocol_version: ProtocolVersion,
    tool_list_changed: Option<ToolListChangedSink>,
) -> McpClientHandler {
    McpClientHandler::new(
        client_info(protocol_version),
        server_id.to_string(),
        tool_list_changed,
    )
}

fn discovery_lifecycle() -> ClientLifecycleMode {
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
    fn stdio_lifecycle_prefers_discovery_and_allows_proven_legacy_fallback() {
        let ClientLifecycleMode::Auto {
            preferred_versions,
            legacy_version,
        } = discovery_lifecycle()
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

    #[test]
    fn http_initialize_retry_only_accepts_closed_discover_response() {
        assert!(should_retry_http_with_initialize(
            &ClientInitializeError::ConnectionClosed("discover response".to_string())
        ));
        assert!(!should_retry_http_with_initialize(
            &ClientInitializeError::ConnectionClosed("initialize response".to_string())
        ));
        assert!(!should_retry_http_with_initialize(
            &ClientInitializeError::Cancelled
        ));
    }
}
