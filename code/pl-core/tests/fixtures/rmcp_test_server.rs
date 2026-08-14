use std::borrow::Cow;
use std::future::Future;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use rmcp::handler::server::ServerHandler;
use rmcp::model::*;
use rmcp::service::RequestContext;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use rmcp::{ErrorData as McpError, RoleServer, ServiceExt};
use serde_json::{Map, json};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
struct FixtureServer {
    transport: &'static str,
}

#[expect(
    clippy::manual_async_fn,
    reason = "RPITIT keeps the required Send bound explicit"
)]
impl ServerHandler for FixtureServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_protocol_version(ProtocolVersion::V_2026_07_28)
            .with_server_info(Implementation::new("pl-rmcp-live-fixture", "1.0.0"))
    }

    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(&[ProtocolVersion::V_2026_07_28])
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, McpError>> + Send + '_ {
        async {
            let mut tool = Tool::new(
                "lookup",
                "Return a structured result from a real rmcp transport.",
                Arc::new(json_object(json!({
                    "type": "object",
                    "properties": { "query": { "type": "string" } },
                    "required": ["query"],
                    "additionalProperties": false
                }))),
            );
            tool.output_schema = Some(Arc::new(json_object(json!({
                "type": "object",
                "properties": {
                    "transport": { "type": "string" },
                    "arguments": { "type": "object" }
                }
            }))));
            tool.annotations = Some(ToolAnnotations::new().read_only(true));
            Ok(ListToolsResult {
                tools: vec![tool],
                ..Default::default()
            })
        }
    }

    fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<CallToolResponse, McpError>> + Send + '_ {
        async move {
            Ok(CallToolResult::structured(json!({
                "transport": self.transport,
                "arguments": request.arguments,
            }))
            .into())
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    write_pid_file(pid_file(&arguments))?;
    write_console_window_file(console_window_file(&arguments))?;
    match arguments.first().map(String::as_str) {
        Some("--stdio") => serve_stdio().await?,
        Some("--http") => {
            let address = arguments
                .get(1)
                .ok_or("--http requires a socket address")?
                .parse::<SocketAddr>()?;
            serve_http(address).await?;
        }
        _ => return Err("expected --stdio or --http <address>".into()),
    }
    Ok(())
}

async fn serve_stdio() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let service = FixtureServer { transport: "stdio" }
        .serve((tokio::io::stdin(), tokio::io::stdout()))
        .await?;
    let _ = service.waiting().await;
    Ok(())
}

async fn serve_http(address: SocketAddr) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let cancellation = CancellationToken::new();
    let config = StreamableHttpServerConfig::default()
        .with_json_response(true)
        .with_sse_keep_alive(None)
        .with_cancellation_token(cancellation.clone());
    let service: StreamableHttpService<FixtureServer, LocalSessionManager> =
        StreamableHttpService::new(
            || {
                Ok(FixtureServer {
                    transport: "streamableHttp",
                })
            },
            Default::default(),
            config,
        );
    let listener = tokio::net::TcpListener::bind(address).await?;
    let router = axum::Router::new().nest_service("/mcp", service);
    let shutdown = cancellation.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        cancellation.cancel();
    });
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown.cancelled_owned())
        .await?;
    Ok(())
}

fn pid_file(arguments: &[String]) -> Option<PathBuf> {
    arguments
        .iter()
        .position(|argument| argument == "--pid-file")
        .and_then(|index| arguments.get(index + 1))
        .map(PathBuf::from)
}

fn write_pid_file(path: Option<PathBuf>) -> std::io::Result<()> {
    if let Some(path) = path {
        std::fs::write(path, std::process::id().to_string())?;
    }
    Ok(())
}

fn console_window_file(arguments: &[String]) -> Option<PathBuf> {
    arguments
        .iter()
        .position(|argument| argument == "--console-window-file")
        .and_then(|index| arguments.get(index + 1))
        .map(PathBuf::from)
}

fn write_console_window_file(path: Option<PathBuf>) -> std::io::Result<()> {
    let Some(path) = path else { return Ok(()) };
    #[cfg(windows)]
    let state = {
        // SAFETY: GetConsoleWindow has no preconditions and only reads this process' console state.
        let window = unsafe { windows::Win32::System::Console::GetConsoleWindow() };
        if window.is_invalid() {
            "none"
        } else {
            "attached"
        }
    };
    #[cfg(not(windows))]
    let state = "unsupported";
    std::fs::write(path, state)
}

fn json_object(value: serde_json::Value) -> Map<String, serde_json::Value> {
    value
        .as_object()
        .expect("fixture schema must be an object")
        .clone()
}
