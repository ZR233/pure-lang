use std::collections::BTreeMap;
use std::fmt;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use pl_protocol::{PureError, Result};
use serde::Serialize;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{Mutex, oneshot};

use crate::config::{EffectiveMcpServerConfig, McpServerConfig, McpServerTransport};
use crate::process::{configure_background_command, terminate_process_tree};

use super::client::{BoxFuture, McpClient};
use super::wire::{JsonRpcRequest, JsonRpcResponse, McpListToolsResult, McpToolDefinition};

const MCP_PROTOCOL_VERSION: &str = "2025-11-25";
pub(super) const PROBE_TIMEOUT: Duration = Duration::from_secs(8);

type PendingRequests = BTreeMap<u64, oneshot::Sender<Result<Value>>>;

pub(super) async fn connect_server(
    server_id: &str,
    server: &EffectiveMcpServerConfig,
) -> Result<Arc<dyn McpClient>> {
    match server.config.transport {
        McpServerTransport::Stdio => {
            let client = StdioMcpClient::spawn(server_id, &server.config).await?;
            Ok(Arc::new(client))
        }
        McpServerTransport::StreamableHttp => {
            let client =
                HttpMcpClient::new(server_id, &server.config, server.bearer_token.clone())?;
            Ok(Arc::new(client))
        }
    }
}

pub(super) async fn initialize_client(client: &Arc<dyn McpClient>) -> Result<()> {
    client
        .request(
            "initialize",
            serde_json::json!({
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {
                    "name": "pure-lang",
                    "version": env!("CARGO_PKG_VERSION"),
                }
            }),
        )
        .await?;
    client
        .notify("notifications/initialized", serde_json::json!({}))
        .await
}

pub(super) async fn list_tools(client: &Arc<dyn McpClient>) -> Result<Vec<McpToolDefinition>> {
    let mut cursor = None;
    let mut tools = Vec::new();
    loop {
        let params = cursor
            .as_ref()
            .map(|cursor| serde_json::json!({ "cursor": cursor }))
            .unwrap_or_else(|| serde_json::json!({}));
        let value = client.request("tools/list", params).await?;
        let result: McpListToolsResult = serde_json::from_value(value)?;
        tools.extend(result.tools);
        cursor = result.next_cursor;
        if cursor.is_none() {
            return Ok(tools);
        }
    }
}

struct StdioMcpClient {
    server_id: String,
    stdin: Mutex<Option<ChildStdin>>,
    child: Mutex<Option<Child>>,
    pending: Arc<Mutex<PendingRequests>>,
    next_id: AtomicU64,
}

impl fmt::Debug for StdioMcpClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StdioMcpClient")
            .field("server_id", &self.server_id)
            .finish_non_exhaustive()
    }
}

impl StdioMcpClient {
    async fn spawn(server_id: &str, server: &McpServerConfig) -> Result<Self> {
        let command = server.command.as_deref().unwrap_or_default();
        let mut process = Command::new(command);
        configure_background_command(&mut process);
        process.args(&server.args);
        process.stdin(Stdio::piped());
        process.stdout(Stdio::piped());
        process.stderr(Stdio::piped());
        if let Some(cwd) = server.cwd.as_deref().filter(|cwd| !cwd.trim().is_empty()) {
            process.current_dir(cwd);
        }
        for (key, value) in &server.env {
            process.env(key, value);
        }
        let child = process.spawn().map_err(|error| {
            PureError::ConfigError(format!(
                "mcp server '{server_id}' failed to start command '{command}': {error}"
            ))
        })?;
        Self::from_child(server_id, child)
    }

    fn from_child(server_id: &str, mut child: Child) -> Result<Self> {
        let stdin = child.stdin.take().ok_or_else(|| {
            PureError::ConfigError(format!("mcp server '{server_id}' stdin is unavailable"))
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            PureError::ConfigError(format!("mcp server '{server_id}' stdout is unavailable"))
        })?;
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    match classify_mcp_stderr_line(&line) {
                        McpStderrSeverity::Info => {}
                        McpStderrSeverity::Warning => {
                            tracing::warn!(message = %line, "MCP server stderr");
                        }
                        McpStderrSeverity::Error => {
                            tracing::error!(message = %line, "MCP server stderr");
                        }
                    }
                }
            });
        }
        let pending = Arc::new(Mutex::new(BTreeMap::new()));
        tokio::spawn(read_stdio_responses(
            server_id.to_string(),
            stdout,
            pending.clone(),
        ));
        Ok(Self {
            server_id: server_id.to_string(),
            stdin: Mutex::new(Some(stdin)),
            child: Mutex::new(Some(child)),
            pending,
            next_id: AtomicU64::new(1),
        })
    }
}

pub(super) fn client_from_stdio_child(server_id: &str, child: Child) -> Result<Arc<dyn McpClient>> {
    Ok(Arc::new(StdioMcpClient::from_child(server_id, child)?))
}

impl McpClient for StdioMcpClient {
    fn request<'a>(&'a self, method: &'a str, params: Value) -> BoxFuture<'a, Result<Value>> {
        Box::pin(async move {
            let id = self.next_id.fetch_add(1, Ordering::Relaxed);
            let (tx, rx) = oneshot::channel();
            self.pending.lock().await.insert(id, tx);
            let mut pending_guard = PendingRequestGuard::new(id, self.pending.clone());
            let request = JsonRpcRequest {
                jsonrpc: "2.0",
                id: Some(id),
                method,
                params,
            };
            let mut stdin_guard = self.stdin.lock().await;
            let Some(stdin) = stdin_guard.as_mut() else {
                self.pending.lock().await.remove(&id);
                return Err(PureError::ToolExecutionFailed {
                    tool: self.server_id.clone(),
                    error: "MCP stdio client is shut down".to_string(),
                });
            };
            if let Err(error) = write_stdio_message(stdin, &request).await {
                self.pending.lock().await.remove(&id);
                return Err(error);
            }
            drop(stdin_guard);
            let result = match rx.await {
                Ok(result) => result,
                Err(_) => Err(PureError::ToolExecutionFailed {
                    tool: self.server_id.clone(),
                    error: "MCP stdio response channel closed".to_string(),
                }),
            };
            pending_guard.complete();
            result
        })
    }

    fn notify<'a>(&'a self, method: &'a str, params: Value) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let request = JsonRpcRequest {
                jsonrpc: "2.0",
                id: None,
                method,
                params,
            };
            let mut stdin_guard = self.stdin.lock().await;
            let Some(stdin) = stdin_guard.as_mut() else {
                return Err(PureError::ToolExecutionFailed {
                    tool: self.server_id.clone(),
                    error: "MCP stdio client is shut down".to_string(),
                });
            };
            write_stdio_message(stdin, &request).await
        })
    }

    fn shutdown<'a>(&'a self) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            self.stdin.lock().await.take();
            {
                let mut pending = self.pending.lock().await;
                let pending = std::mem::take(&mut *pending);
                for (_, sender) in pending {
                    let _ = sender.send(Err(PureError::ToolExecutionFailed {
                        tool: self.server_id.clone(),
                        error: "MCP stdio client shut down".to_string(),
                    }));
                }
            }
            let Some(mut child) = self.child.lock().await.take() else {
                return;
            };
            let pid = child.id();
            if tokio::time::timeout(Duration::from_millis(500), child.wait())
                .await
                .is_err()
            {
                terminate_process_tree(pid).await;
                let _ = child.start_kill();
                let _ = child.wait().await;
            }
        })
    }
}

async fn read_stdio_responses<R>(
    server_id: String,
    stdout: R,
    pending: Arc<Mutex<BTreeMap<u64, oneshot::Sender<Result<Value>>>>>,
) where
    R: AsyncRead + Unpin,
{
    let mut lines = BufReader::new(stdout).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let Ok(response) = serde_json::from_str::<JsonRpcResponse>(&line) else {
            tracing::warn!(
                server_id,
                response = %line,
                "MCP server returned invalid JSON"
            );
            continue;
        };
        let Some(id) = response.id else {
            continue;
        };
        let result = json_rpc_response_result(response);
        if let Some(sender) = pending.lock().await.remove(&id) {
            let _ = sender.send(result);
        }
    }
    let pending_requests = std::mem::take(&mut *pending.lock().await);
    for (_, sender) in pending_requests {
        let _ = sender.send(Err(PureError::ToolExecutionFailed {
            tool: server_id.clone(),
            error: "MCP stdio transport closed before returning a response".to_string(),
        }));
    }
}

async fn write_stdio_message<T: Serialize>(stdin: &mut ChildStdin, value: &T) -> Result<()> {
    let message = serde_json::to_string(value)?;
    stdin.write_all(message.as_bytes()).await?;
    stdin.write_all(b"\n").await?;
    stdin.flush().await?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum McpStderrSeverity {
    Info,
    Warning,
    Error,
}

pub(super) fn classify_mcp_stderr_line(line: &str) -> McpStderrSeverity {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return McpStderrSeverity::Info;
    }
    if let Ok(value) = serde_json::from_str::<Value>(trimmed)
        && let Some(level) = value.get("level").and_then(Value::as_str)
    {
        return mcp_stderr_severity_from_level(level);
    }
    if let Some(level) = extract_bracketed_mcp_log_level(trimmed) {
        return mcp_stderr_severity_from_level(level);
    }
    let upper = trimmed.to_ascii_uppercase();
    if upper.contains(" ERROR") || upper.starts_with("ERROR") {
        return McpStderrSeverity::Error;
    }
    if upper.contains(" WARN") || upper.starts_with("WARN") {
        return McpStderrSeverity::Warning;
    }
    McpStderrSeverity::Error
}

fn extract_bracketed_mcp_log_level(line: &str) -> Option<&str> {
    let marker = "] ";
    let index = line.find(marker)?;
    line[index + marker.len()..]
        .split_once(':')
        .map(|(level, _)| level.trim())
}

fn mcp_stderr_severity_from_level(level: &str) -> McpStderrSeverity {
    match level.to_ascii_uppercase().as_str() {
        "TRACE" | "DEBUG" | "INFO" => McpStderrSeverity::Info,
        "WARN" | "WARNING" => McpStderrSeverity::Warning,
        "ERROR" | "FATAL" => McpStderrSeverity::Error,
        _ => McpStderrSeverity::Error,
    }
}

pub(super) struct HttpMcpClient {
    server_id: String,
    url: String,
    client: reqwest::Client,
    headers: BTreeMap<String, String>,
    pub(super) bearer_token: Option<String>,
    session_id: Mutex<Option<String>>,
    next_id: AtomicU64,
}

impl fmt::Debug for HttpMcpClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpMcpClient")
            .field("server_id", &self.server_id)
            .field("url", &self.url)
            .finish_non_exhaustive()
    }
}

impl HttpMcpClient {
    pub(super) fn new(
        server_id: &str,
        server: &McpServerConfig,
        bearer_token_override: Option<String>,
    ) -> Result<Self> {
        let bearer_token = match bearer_token_override {
            Some(token) => Some(token),
            None => match server.bearer_token_env_var.as_deref() {
                Some(env_var) if !env_var.trim().is_empty() => Some(std::env::var(env_var).map_err(
                    |error| {
                        PureError::ConfigError(format!(
                            "mcp server '{server_id}' bearer token env var '{env_var}' is unavailable: {error}"
                        ))
                    },
                )?),
                Some(_) | None => None,
            },
        };
        Ok(Self {
            server_id: server_id.to_string(),
            url: server.url.clone().unwrap_or_default(),
            client: reqwest::Client::new(),
            headers: server.headers.clone(),
            bearer_token,
            session_id: Mutex::new(None),
            next_id: AtomicU64::new(1),
        })
    }

    async fn send_http_rpc(&self, payload: Value) -> Result<Option<Value>> {
        let mut request = self
            .client
            .post(&self.url)
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream");
        for (key, value) in &self.headers {
            request = request.header(key, value);
        }
        if let Some(token) = &self.bearer_token {
            request = request.bearer_auth(token);
        }
        if let Some(session_id) = self.session_id.lock().await.clone() {
            request = request.header("mcp-session-id", session_id);
        }
        let response = request.json(&payload).send().await.map_err(|error| {
            PureError::HttpError(format!(
                "mcp server '{}' request failed: {error}",
                self.server_id
            ))
        })?;
        if let Some(session_id) = response
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
        {
            *self.session_id.lock().await = Some(session_id.to_string());
        }
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(PureError::HttpError(format!(
                "mcp server '{}' returned HTTP {status}: {text}",
                self.server_id
            )));
        }
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let text = response.text().await.map_err(|error| {
            PureError::HttpError(format!(
                "mcp server '{}' response read failed: {error}",
                self.server_id
            ))
        })?;
        if text.trim().is_empty() {
            return Ok(None);
        }
        if content_type.contains("text/event-stream")
            || text.lines().any(|line| line.starts_with("data:"))
        {
            return Ok(Some(parse_sse_json(&text)?));
        }
        let response = serde_json::from_str::<JsonRpcResponse>(&text)?;
        json_rpc_response_result(response).map(Some)
    }
}

impl McpClient for HttpMcpClient {
    fn request<'a>(&'a self, method: &'a str, params: Value) -> BoxFuture<'a, Result<Value>> {
        Box::pin(async move {
            let id = self.next_id.fetch_add(1, Ordering::Relaxed);
            let payload = serde_json::to_value(JsonRpcRequest {
                jsonrpc: "2.0",
                id: Some(id),
                method,
                params,
            })?;
            match self.send_http_rpc(payload).await {
                Ok(Some(value)) => Ok(value),
                Ok(None) => Err(PureError::ToolExecutionFailed {
                    tool: self.server_id.clone(),
                    error: "MCP HTTP request returned no JSON-RPC response".to_string(),
                }),
                Err(error) => Err(error),
            }
        })
    }

    fn notify<'a>(&'a self, method: &'a str, params: Value) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let payload = serde_json::to_value(JsonRpcRequest {
                jsonrpc: "2.0",
                id: None,
                method,
                params,
            })?;
            self.send_http_rpc(payload).await.map(|_| ())
        })
    }

    fn shutdown<'a>(&'a self) -> BoxFuture<'a, ()> {
        Box::pin(async {})
    }
}

struct PendingRequestGuard {
    id: u64,
    pending: Arc<Mutex<PendingRequests>>,
    completed: bool,
}

impl PendingRequestGuard {
    fn new(id: u64, pending: Arc<Mutex<PendingRequests>>) -> Self {
        Self {
            id,
            pending,
            completed: false,
        }
    }

    fn complete(&mut self) {
        self.completed = true;
    }
}

impl Drop for PendingRequestGuard {
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        if let Ok(mut pending) = self.pending.try_lock() {
            pending.remove(&self.id);
            return;
        }
        let id = self.id;
        let pending = self.pending.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                pending.lock().await.remove(&id);
            });
        }
    }
}

fn json_rpc_response_result(response: JsonRpcResponse) -> Result<Value> {
    if let Some(error) = response.error {
        let code = error.code;
        let message = &error.message;
        return Err(PureError::ToolExecutionFailed {
            tool: "mcp".to_string(),
            error: format!("JSON-RPC error {code}: {message}"),
        });
    }
    response
        .result
        .ok_or_else(|| PureError::ToolExecutionFailed {
            tool: "mcp".to_string(),
            error: "JSON-RPC response missing result".to_string(),
        })
}

fn parse_sse_json(text: &str) -> Result<Value> {
    let data = sse_data_messages(text)
        .into_iter()
        .find(|message| {
            let trimmed = message.trim();
            !trimmed.is_empty() && trimmed != "[DONE]"
        })
        .ok_or_else(|| PureError::HttpError("MCP SSE response did not contain data".to_string()))?;
    let response = serde_json::from_str::<JsonRpcResponse>(data.trim())?;
    json_rpc_response_result(response)
}

fn sse_data_messages(text: &str) -> Vec<String> {
    let mut messages = Vec::new();
    let mut current = Vec::new();
    for line in text.lines() {
        if line.is_empty() {
            if !current.is_empty() {
                messages.push(current.join("\n"));
                current.clear();
            }
            continue;
        }
        if let Some(data) = line.strip_prefix("data:") {
            current.push(data.strip_prefix(' ').unwrap_or(data).to_string());
        }
    }
    if !current.is_empty() {
        messages.push(current.join("\n"));
    }
    messages
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use pretty_assertions::assert_eq;
    use tokio::sync::{Mutex, oneshot};

    use super::{PendingRequestGuard, parse_sse_json};

    #[tokio::test]
    async fn dropping_pending_request_guard_removes_cancelled_request() {
        let pending = Arc::new(Mutex::new(BTreeMap::new()));
        let (sender, _receiver) = oneshot::channel();
        pending.lock().await.insert(7, sender);

        drop(PendingRequestGuard::new(7, pending.clone()));
        tokio::task::yield_now().await;

        assert!(pending.lock().await.is_empty());
    }

    #[tokio::test]
    async fn stdio_eof_releases_every_pending_request() {
        let pending = Arc::new(Mutex::new(BTreeMap::new()));
        let (sender, receiver) = oneshot::channel();
        pending.lock().await.insert(9, sender);

        super::read_stdio_responses(
            "closed-server".to_string(),
            tokio::io::empty(),
            pending.clone(),
        )
        .await;

        let error = receiver.await.unwrap().unwrap_err().to_string();
        assert!(error.contains("transport closed"));
        assert!(pending.lock().await.is_empty());
    }

    #[test]
    fn parse_sse_json_reads_single_data_message() {
        let value = parse_sse_json(
            "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"ok\":true}}\n\n",
        )
        .unwrap();

        assert_eq!(value, serde_json::json!({ "ok": true }));
    }

    #[test]
    fn parse_sse_json_joins_multiline_data_message() {
        let value = parse_sse_json(
            "event: message\n\
             data: {\"jsonrpc\":\"2.0\",\n\
             data: \"id\":1,\n\
             data: \"result\":{\"ok\":true}}\n\n",
        )
        .unwrap();

        assert_eq!(value, serde_json::json!({ "ok": true }));
    }
}
