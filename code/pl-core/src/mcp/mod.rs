use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::pin::Pin;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use futures::Future;
use pl_protocol::{PureError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{Mutex, oneshot};

use crate::config::{
    EffectiveMcpServerConfig, McpServerConfig, McpServerStatusKind, McpServerTransport,
    validate_mcp_identifier,
};
use crate::tool::{OutputTruncation, Tool, ToolContext, ToolInput, ToolOutput};

const MCP_PROTOCOL_VERSION: &str = "2025-11-25";
const MCP_TOOL_PREFIX: &str = "mcp__";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// MCP server 的 JSON-RPC 请求抽象。
///
/// 具体 transport 实现负责连接、请求/响应匹配和生命周期资源持有；
/// tool 适配器只依赖此 trait 发送 `tools/call`。
trait McpClient: fmt::Debug + Send + Sync {
    fn request<'a>(&'a self, method: &'a str, params: Value) -> BoxFuture<'a, Result<Value>>;
    fn notify<'a>(&'a self, method: &'a str, params: Value) -> BoxFuture<'a, Result<()>>;
}

#[derive(Debug, Clone)]
pub(crate) struct McpToolAdapter {
    exposed_name: String,
    raw_name: String,
    description: String,
    input_schema: Value,
    client: Arc<dyn McpClient>,
}

impl McpToolAdapter {
    fn new(
        server_id: &str,
        definition: McpToolDefinition,
        client: Arc<dyn McpClient>,
    ) -> Result<Self> {
        let exposed_name = exposed_tool_name(server_id, &definition.name)?;
        Ok(Self {
            exposed_name,
            raw_name: definition.name,
            description: definition.description.unwrap_or_default(),
            input_schema: definition.input_schema,
            client,
        })
    }
}

impl Tool for McpToolAdapter {
    fn name(&self) -> &str {
        &self.exposed_name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        self.input_schema.clone()
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        _context: ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolOutput>> + Send + 'a>> {
        Box::pin(async move {
            let params = serde_json::json!({
                "name": self.raw_name,
                "arguments": input.arguments,
            });
            let value = self.client.request("tools/call", params).await?;
            let result: McpCallToolResult = serde_json::from_value(value)?;
            if result.is_error {
                return Err(PureError::ToolExecutionFailed {
                    tool: self.exposed_name.clone(),
                    error: format_mcp_content(&result.content),
                });
            }
            Ok(ToolOutput {
                description: format_mcp_content(&result.content),
                truncated: OutputTruncation::empty(),
                output_file: PathBuf::new(),
                exit_code: None,
                timed_out: false,
            })
        })
    }
}

pub(crate) async fn register_configured_mcp_tools(
    core: &mut crate::PureCore,
    servers: &BTreeMap<String, EffectiveMcpServerConfig>,
) -> Result<()> {
    for (server_id, server) in servers
        .iter()
        .filter(|(_, server)| server.status_kind == McpServerStatusKind::Enabled)
    {
        let client = connect_server(server_id, server).await?;
        initialize_client(&client).await?;
        let tools = list_tools(&client).await?;
        for definition in tools {
            let adapter = McpToolAdapter::new(server_id, definition, client.clone())?;
            if core.has_tool(adapter.name()) {
                return Err(PureError::ConfigError(format!(
                    "mcp tool '{}' conflicts with an existing tool",
                    adapter.name()
                )));
            }
            core.register_tool(adapter);
        }
    }
    Ok(())
}

pub(crate) fn is_mcp_tool_name(name: &str) -> bool {
    name.starts_with(MCP_TOOL_PREFIX)
}

fn exposed_tool_name(server_id: &str, tool_name: &str) -> Result<String> {
    validate_mcp_identifier(server_id, "MCP server id")?;
    validate_mcp_identifier(tool_name, "MCP tool name")?;
    Ok(format!("{MCP_TOOL_PREFIX}{server_id}__{tool_name}"))
}

async fn connect_server(
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

async fn initialize_client(client: &Arc<dyn McpClient>) -> Result<()> {
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

async fn list_tools(client: &Arc<dyn McpClient>) -> Result<Vec<McpToolDefinition>> {
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
    stdin: Mutex<ChildStdin>,
    _child: Mutex<Child>,
    pending: Arc<Mutex<BTreeMap<u64, oneshot::Sender<Result<Value>>>>>,
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
        let mut child = process.spawn().map_err(|error| {
            PureError::ConfigError(format!(
                "mcp server '{server_id}' failed to start command '{command}': {error}"
            ))
        })?;
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
                    eprintln!("[pl-core] mcp stderr: {line}");
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
            stdin: Mutex::new(stdin),
            _child: Mutex::new(child),
            pending,
            next_id: AtomicU64::new(1),
        })
    }
}

impl McpClient for StdioMcpClient {
    fn request<'a>(&'a self, method: &'a str, params: Value) -> BoxFuture<'a, Result<Value>> {
        Box::pin(async move {
            let id = self.next_id.fetch_add(1, Ordering::Relaxed);
            let (tx, rx) = oneshot::channel();
            self.pending.lock().await.insert(id, tx);
            let request = JsonRpcRequest {
                jsonrpc: "2.0",
                id: Some(id),
                method,
                params,
            };
            let mut stdin = self.stdin.lock().await;
            if let Err(error) = write_stdio_message(&mut stdin, &request).await {
                self.pending.lock().await.remove(&id);
                return Err(error);
            }
            match tokio::time::timeout(REQUEST_TIMEOUT, rx).await {
                Ok(Ok(result)) => result,
                Ok(Err(_)) => Err(PureError::ToolExecutionFailed {
                    tool: self.server_id.clone(),
                    error: "MCP stdio response channel closed".to_string(),
                }),
                Err(_) => {
                    self.pending.lock().await.remove(&id);
                    Err(PureError::ToolExecutionFailed {
                        tool: self.server_id.clone(),
                        error: "MCP stdio request timed out".to_string(),
                    })
                }
            }
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
            let mut stdin = self.stdin.lock().await;
            write_stdio_message(&mut stdin, &request).await
        })
    }
}

async fn read_stdio_responses(
    server_id: String,
    stdout: tokio::process::ChildStdout,
    pending: Arc<Mutex<BTreeMap<u64, oneshot::Sender<Result<Value>>>>>,
) {
    let mut lines = BufReader::new(stdout).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let Ok(response) = serde_json::from_str::<JsonRpcResponse>(&line) else {
            eprintln!("[pl-core] mcp server '{server_id}' returned invalid JSON: {line}");
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
}

async fn write_stdio_message<T: Serialize>(stdin: &mut ChildStdin, value: &T) -> Result<()> {
    let message = serde_json::to_string(value)?;
    stdin.write_all(message.as_bytes()).await?;
    stdin.write_all(b"\n").await?;
    stdin.flush().await?;
    Ok(())
}

struct HttpMcpClient {
    server_id: String,
    url: String,
    client: reqwest::Client,
    headers: BTreeMap<String, String>,
    bearer_token: Option<String>,
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
    fn new(
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
            match tokio::time::timeout(REQUEST_TIMEOUT, self.send_http_rpc(payload)).await {
                Ok(Ok(Some(value))) => Ok(value),
                Ok(Ok(None)) => Err(PureError::ToolExecutionFailed {
                    tool: self.server_id.clone(),
                    error: "MCP HTTP request returned no JSON-RPC response".to_string(),
                }),
                Ok(Err(error)) => Err(error),
                Err(_) => Err(PureError::ToolExecutionFailed {
                    tool: self.server_id.clone(),
                    error: "MCP HTTP request timed out".to_string(),
                }),
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
            match tokio::time::timeout(REQUEST_TIMEOUT, self.send_http_rpc(payload)).await {
                Ok(Ok(_)) => Ok(()),
                Ok(Err(error)) => Err(error),
                Err(_) => Err(PureError::ToolExecutionFailed {
                    tool: self.server_id.clone(),
                    error: "MCP HTTP notification timed out".to_string(),
                }),
            }
        })
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonRpcRequest<'a> {
    jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<u64>,
    method: &'a str,
    params: Value,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    id: Option<u64>,
    result: Option<Value>,
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct McpListToolsResult {
    #[serde(default)]
    tools: Vec<McpToolDefinition>,
    next_cursor: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct McpToolDefinition {
    name: String,
    description: Option<String>,
    #[serde(default = "default_input_schema")]
    input_schema: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct McpCallToolResult {
    #[serde(default)]
    content: Vec<Value>,
    #[serde(default)]
    is_error: bool,
}

fn json_rpc_response_result(response: JsonRpcResponse) -> Result<Value> {
    if let Some(error) = response.error {
        return Err(PureError::ToolExecutionFailed {
            tool: "mcp".to_string(),
            error: format!("JSON-RPC error {}: {}", error.code, error.message),
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
    let data = text
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(str::trim)
        .find(|line| !line.is_empty() && *line != "[DONE]")
        .ok_or_else(|| PureError::HttpError("MCP SSE response did not contain data".to_string()))?;
    let response = serde_json::from_str::<JsonRpcResponse>(data)?;
    json_rpc_response_result(response)
}

fn format_mcp_content(content: &[Value]) -> String {
    if content.is_empty() {
        return String::new();
    }
    let parts = content
        .iter()
        .map(format_mcp_content_part)
        .collect::<Vec<_>>();
    parts.join("\n")
}

fn format_mcp_content_part(content: &Value) -> String {
    let Some(object) = content.as_object() else {
        return compact_json(content);
    };
    match object.get("type").and_then(Value::as_str) {
        Some("text") => object
            .get("text")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| compact_json(content)),
        Some("json") => object
            .get("json")
            .map(compact_json)
            .unwrap_or_else(|| compact_json(content)),
        _ => compact_json(&Value::Object(object.clone())),
    }
}

fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| value.to_string())
}

fn default_input_schema() -> Value {
    let mut map = Map::new();
    map.insert("type".to_string(), Value::String("object".to_string()));
    map.insert("properties".to_string(), Value::Object(Map::new()));
    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposed_tool_name_prefixes_server_and_tool() {
        let name = exposed_tool_name("github", "search_issues").unwrap();

        assert_eq!(name, "mcp__github__search_issues");
        assert!(is_mcp_tool_name(&name));
    }

    #[test]
    fn exposed_tool_name_rejects_invalid_raw_tool() {
        let error = exposed_tool_name("github", "bad tool").unwrap_err();

        assert!(error.to_string().contains("MCP tool name"));
    }

    #[test]
    fn format_mcp_content_prefers_text_parts() {
        let content = vec![
            serde_json::json!({"type": "text", "text": "hello"}),
            serde_json::json!({"type": "json", "json": {"ok": true}}),
        ];

        assert_eq!(format_mcp_content(&content), "hello\n{\"ok\":true}");
    }

    #[test]
    fn http_client_uses_bearer_token_override() {
        let server = McpServerConfig {
            transport: McpServerTransport::StreamableHttp,
            url: Some("https://example.com/mcp".to_string()),
            bearer_token_env_var: Some("IGNORED_ENV_VAR".to_string()),
            ..Default::default()
        };

        let client =
            HttpMcpClient::new("zhipu_search", &server, Some("coding-plan-key".to_string()))
                .unwrap();

        assert_eq!(client.bearer_token.as_deref(), Some("coding-plan-key"));
    }
}
