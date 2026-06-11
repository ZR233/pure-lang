use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::{Mutex, broadcast};

use crate::client::{
    DiagnosticSink, LspClient, LspServerDefinition, with_content_modified_retries,
};
use crate::formatting::{format_diagnostics, format_lsp_result};
use crate::process::{configure_background_command, terminate_process_tree};
use crate::types::{
    LanguageToolInfo, LspActivityKind, LspAvailabilityKind, LspDiagnostic, LspQuery,
    LspQueryOperation, LspQueryResult, LspResult, LspRuntimeError, LspServerSnapshot,
};
use crate::uri::path_to_file_uri;

const RUST_ANALYZER_ID: &str = "rust-analyzer";
const RUST_ANALYZER_COMMAND: &str = "rust-analyzer";
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const RUSTUP_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Clone)]
pub struct LspRuntimeRegistry {
    state: Arc<Mutex<LspRuntimeState>>,
    diagnostics: Arc<Mutex<HashMap<String, Vec<LspDiagnostic>>>>,
    updates: broadcast::Sender<()>,
}

impl std::fmt::Debug for LspRuntimeRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LspRuntimeRegistry").finish_non_exhaustive()
    }
}

impl Default for LspRuntimeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl LspRuntimeRegistry {
    pub fn new() -> Self {
        let (updates, _) = broadcast::channel(64);
        Self {
            state: Arc::new(Mutex::new(LspRuntimeState::default())),
            diagnostics: Arc::new(Mutex::new(HashMap::new())),
            updates,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.updates.subscribe()
    }

    pub async fn reconcile_workspace(&self, workspace_root: impl AsRef<Path>) {
        self.reconcile_workspace_with_command(workspace_root.as_ref(), RUST_ANALYZER_COMMAND)
            .await;
    }

    pub async fn active_server_names(&self) -> Vec<String> {
        self.state
            .lock()
            .await
            .servers
            .iter()
            .filter(|(_, server)| server.availability_kind == LspAvailabilityKind::Available)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// 返回当前 Available 状态的所有语言工具信息。
    ///
    /// 每种已注册且可用的 LSP 服务器会按其 `language_ids` 逐一展开，
    /// 便于 `pl-core` 为每个语言生成独立的查询工具。
    pub async fn available_languages(&self) -> Vec<LanguageToolInfo> {
        let state = self.state.lock().await;
        let mut result = Vec::new();
        for server in state.servers.values() {
            if server.availability_kind != LspAvailabilityKind::Available {
                continue;
            }
            for language_id in &server.definition.language_ids {
                result.push(LanguageToolInfo {
                    language_id: language_id.clone(),
                    server_id: server.definition.id.clone(),
                    display_name: server.definition.display_name.clone(),
                    extensions: extensions_for_language(&server.definition, language_id),
                });
            }
        }
        result
    }

    pub async fn snapshots(&self) -> Vec<LspServerSnapshot> {
        let diagnostics = self.diagnostics.lock().await;
        let diagnostic_counts = diagnostic_counts(&diagnostics);
        drop(diagnostics);
        let snapshots = self
            .state
            .lock()
            .await
            .servers
            .values()
            .map(|server| {
                (
                    server.snapshot(*diagnostic_counts.get(&server.definition.id).unwrap_or(&0)),
                    server.client.clone(),
                )
            })
            .collect::<Vec<_>>();
        let mut output = Vec::with_capacity(snapshots.len());
        for (mut snapshot, client) in snapshots {
            if let Some(client) = client {
                let status = client.runtime_status().await;
                snapshot.activity_kind = status.activity_kind;
                snapshot.activity_title = status.activity_title;
                snapshot.activity_message = status.activity_message;
                snapshot.activity_percentage = status.activity_percentage;
                snapshot.last_error = status.last_error;
                snapshot.last_error_at = status.last_error_at;
            }
            output.push(snapshot);
        }
        output
    }

    pub async fn query(&self, query: LspQuery) -> LspResult<LspQueryResult> {
        if query.operation == LspQueryOperation::Diagnostics {
            return self.query_diagnostics(query).await;
        }
        let (server_id, definition, client) = self.client_for_query(&query).await?;
        if let Some(path) = query.file_path.as_deref() {
            client.open_or_change_file(path).await?;
        }
        let value = request_for_query(&client, &query).await?;
        let formatted = format_lsp_result(query.operation, &value, &definition.workspace_root);
        Ok(LspQueryResult {
            success: true,
            operation: query.operation,
            server_id: Some(server_id),
            result: formatted.text,
            result_count: formatted.result_count,
            file_count: formatted.file_count,
        })
    }

    pub async fn notify_file_changed(&self, path: impl AsRef<Path>) {
        let path = path.as_ref();
        for client in self.open_clients().await {
            let _ = client.notify_watched_file_changed(path).await;
        }
        let Some(client) = self.open_client_for_path(path).await else {
            return;
        };
        if path.is_file() {
            if client.open_or_change_file(path).await.is_ok() {
                let _ = client.save_file(path).await;
            }
        } else {
            let _ = client.close_file(path).await;
        }
    }

    pub async fn notify_file_deleted(&self, path: impl AsRef<Path>) {
        let path = path.as_ref();
        for client in self.open_clients().await {
            let _ = client.notify_watched_file_deleted(path).await;
        }
        if let Some(client) = self.open_client_for_path(path).await {
            let _ = client.close_file(path).await;
        }
    }

    pub async fn shutdown(&self) {
        let clients = {
            let mut state = self.state.lock().await;
            state.workspace_root = None;
            let clients = state
                .servers
                .values_mut()
                .filter_map(|server| server.client.take())
                .collect::<Vec<_>>();
            state.servers.clear();
            clients
        };
        self.diagnostics.lock().await.clear();
        for client in clients {
            client.shutdown().await;
        }
        self.emit_update();
    }

    async fn reconcile_workspace_with_command(&self, workspace_root: &Path, command: &str) {
        let workspace_root =
            std::fs::canonicalize(workspace_root).unwrap_or_else(|_| workspace_root.to_path_buf());
        let old_clients = {
            let mut state = self.state.lock().await;
            let workspace_changed = state.workspace_root.as_ref() != Some(&workspace_root);
            if workspace_changed {
                let clients = state
                    .servers
                    .values()
                    .filter_map(|server| server.client.clone())
                    .collect::<Vec<_>>();
                state.servers.clear();
                self.diagnostics.lock().await.clear();
                state.workspace_root = Some(workspace_root.clone());
                clients
            } else {
                Vec::new()
            }
        };
        for client in old_clients {
            client.shutdown().await;
        }

        let definition = rust_analyzer_definition(&workspace_root, command);
        let next = if !workspace_root.join("Cargo.toml").exists() {
            LspRuntimeServerState::new(
                definition,
                LspAvailabilityKind::Disabled,
                Some("No Cargo.toml found in workspace root".to_string()),
                None,
            )
        } else {
            match probe_rust_analyzer(command).await {
                Ok(version) => LspRuntimeServerState::new(
                    definition,
                    LspAvailabilityKind::Available,
                    Some(version),
                    Some(unix_seconds()),
                ),
                Err(ProbeError::MissingCommand) => LspRuntimeServerState::new(
                    definition,
                    LspAvailabilityKind::MissingCommand,
                    Some(missing_rust_analyzer_message()),
                    Some(unix_seconds()),
                ),
                Err(ProbeError::MissingRustupComponent) => LspRuntimeServerState::new(
                    definition,
                    LspAvailabilityKind::MissingCommand,
                    Some(missing_rust_analyzer_message()),
                    Some(unix_seconds()),
                ),
                Err(ProbeError::Failed(message)) => LspRuntimeServerState::new(
                    definition,
                    LspAvailabilityKind::Unavailable,
                    Some(message),
                    Some(unix_seconds()),
                ),
            }
        };
        {
            let mut state = self.state.lock().await;
            state.servers.insert(RUST_ANALYZER_ID.to_string(), next);
        }
        self.emit_update();
    }

    async fn client_for_query(
        &self,
        query: &LspQuery,
    ) -> LspResult<(String, LspServerDefinition, Arc<LspClient>)> {
        let server_id = self.server_id_for_query(query).await?;
        let mut state = self.state.lock().await;
        let server = state.servers.get_mut(&server_id).ok_or_else(|| {
            LspRuntimeError::Unavailable(format!("LSP server not configured: {server_id}"))
        })?;
        if server.availability_kind != LspAvailabilityKind::Available {
            return Err(LspRuntimeError::Unavailable(
                server
                    .availability_message
                    .clone()
                    .unwrap_or_else(|| format!("{server_id} is not available")),
            ));
        }
        let definition = server.definition.clone();
        let client = match &server.client {
            Some(client) => client.clone(),
            None => {
                let sink = DiagnosticSink::new(
                    definition.id.clone(),
                    definition.workspace_root.clone(),
                    self.diagnostics.clone(),
                    self.updates.clone(),
                );
                let client = Arc::new(LspClient::new(definition.clone(), sink));
                server.client = Some(client.clone());
                client
            }
        };
        Ok((server_id, definition, client))
    }

    async fn server_id_for_query(&self, query: &LspQuery) -> LspResult<String> {
        if let Some(language_id) = query.language_id.as_deref() {
            return self
                .server_id_for_language(language_id)
                .await
                .ok_or_else(|| {
                    LspRuntimeError::Unavailable(format!(
                        "No LSP server configured for language: {language_id}"
                    ))
                });
        }
        if query.operation.requires_file() {
            let path = query
                .file_path
                .as_deref()
                .ok_or_else(|| LspRuntimeError::InvalidQuery("filePath is required".to_string()))?;
            self.server_id_for_path(path).await.ok_or_else(|| {
                LspRuntimeError::Unavailable(format!(
                    "No LSP server available for {}",
                    path.display()
                ))
            })
        } else {
            self.active_server_names()
                .await
                .into_iter()
                .next()
                .ok_or_else(|| LspRuntimeError::Unavailable("No active LSP servers".to_string()))
        }
    }

    async fn server_id_for_language(&self, language_id: &str) -> Option<String> {
        self.state
            .lock()
            .await
            .servers
            .iter()
            .find(|(_, server)| {
                server
                    .definition
                    .language_ids
                    .iter()
                    .any(|item| item == language_id)
            })
            .map(|(server_id, _)| server_id.clone())
    }

    async fn server_id_for_path(&self, path: &Path) -> Option<String> {
        let extension = path.extension()?.to_string_lossy().to_ascii_lowercase();
        let extension = format!(".{extension}");
        self.state
            .lock()
            .await
            .servers
            .iter()
            .find(|(_, server)| {
                server
                    .definition
                    .extensions
                    .iter()
                    .any(|item| item == &extension)
            })
            .map(|(server_id, _)| server_id.clone())
    }

    async fn open_client_for_path(&self, path: &Path) -> Option<Arc<LspClient>> {
        let server_id = self.server_id_for_path(path).await?;
        self.state
            .lock()
            .await
            .servers
            .get(&server_id)
            .and_then(|server| server.client.clone())
    }

    async fn open_clients(&self) -> Vec<Arc<LspClient>> {
        self.state
            .lock()
            .await
            .servers
            .values()
            .filter_map(|server| server.client.clone())
            .collect()
    }

    async fn query_diagnostics(&self, query: LspQuery) -> LspResult<LspQueryResult> {
        let max_results = query.max_results.unwrap_or(100);
        let mut diagnostics = self.all_diagnostics().await;
        let server_id = if query.language_id.is_some() {
            Some(self.server_id_for_query(&query).await?)
        } else {
            None
        };
        if let Some(server_id) = server_id.as_deref() {
            diagnostics.retain(|diagnostic| diagnostic.server_id == server_id);
        }
        if let Some(path) = query.file_path.as_deref() {
            let display = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_string();
            diagnostics.retain(|diagnostic| {
                diagnostic.path == path.to_string_lossy() || diagnostic.path.ends_with(&display)
            });
        }
        let formatted = format_diagnostics(&diagnostics, max_results);
        Ok(LspQueryResult {
            success: true,
            operation: LspQueryOperation::Diagnostics,
            server_id,
            result: formatted.text,
            result_count: formatted.result_count,
            file_count: formatted.file_count,
        })
    }

    async fn all_diagnostics(&self) -> Vec<LspDiagnostic> {
        let mut diagnostics = self
            .diagnostics
            .lock()
            .await
            .values()
            .flat_map(|items| items.iter().cloned())
            .collect::<Vec<_>>();
        diagnostics.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then(left.range.start.line.cmp(&right.range.start.line))
                .then(left.range.start.character.cmp(&right.range.start.character))
        });
        diagnostics
    }

    fn emit_update(&self) {
        let _ = self.updates.send(());
    }
}

#[derive(Default)]
struct LspRuntimeState {
    workspace_root: Option<PathBuf>,
    servers: BTreeMap<String, LspRuntimeServerState>,
}

struct LspRuntimeServerState {
    definition: LspServerDefinition,
    availability_kind: LspAvailabilityKind,
    availability_message: Option<String>,
    last_checked_at: Option<i64>,
    client: Option<Arc<LspClient>>,
}

impl LspRuntimeServerState {
    fn new(
        definition: LspServerDefinition,
        availability_kind: LspAvailabilityKind,
        availability_message: Option<String>,
        last_checked_at: Option<i64>,
    ) -> Self {
        Self {
            definition,
            availability_kind,
            availability_message,
            last_checked_at,
            client: None,
        }
    }

    fn snapshot(&self, diagnostic_count: usize) -> LspServerSnapshot {
        LspServerSnapshot {
            id: self.definition.id.clone(),
            display_name: self.definition.display_name.clone(),
            extensions: self.definition.extensions.clone(),
            language_ids: self.definition.language_ids.clone(),
            availability_kind: self.availability_kind,
            availability_message: self.availability_message.clone(),
            last_checked_at: self.last_checked_at,
            diagnostic_count,
            activity_kind: LspActivityKind::Idle,
            activity_title: None,
            activity_message: None,
            activity_percentage: None,
            last_error: None,
            last_error_at: None,
        }
    }
}

async fn request_for_query(client: &Arc<LspClient>, query: &LspQuery) -> LspResult<Value> {
    let (method, params) = method_and_params(query)?;
    let mut value = with_content_modified_retries(|| {
        let client = client.clone();
        let method = method.clone();
        let params = params.clone();
        async move { client.request(&method, params).await }
    })
    .await?;
    if matches!(
        query.operation,
        LspQueryOperation::IncomingCalls | LspQueryOperation::OutgoingCalls
    ) {
        let Some(items) = value.as_array() else {
            return Ok(Value::Array(Vec::new()));
        };
        let Some(item) = items.first() else {
            return Ok(Value::Array(Vec::new()));
        };
        let method = match query.operation {
            LspQueryOperation::IncomingCalls => "callHierarchy/incomingCalls",
            LspQueryOperation::OutgoingCalls => "callHierarchy/outgoingCalls",
            _ => unreachable!(),
        };
        value = with_content_modified_retries(|| {
            let client = client.clone();
            let item = item.clone();
            async move {
                client
                    .request(method, serde_json::json!({ "item": item }))
                    .await
            }
        })
        .await?;
    }
    Ok(value)
}

fn method_and_params(query: &LspQuery) -> LspResult<(String, Value)> {
    let uri = query
        .file_path
        .as_deref()
        .map(path_to_file_uri)
        .unwrap_or_default();
    let position = if query.operation.requires_position() {
        let line = query
            .line
            .ok_or_else(|| LspRuntimeError::InvalidQuery("line is required".to_string()))?;
        let character = query
            .character
            .ok_or_else(|| LspRuntimeError::InvalidQuery("character is required".to_string()))?;
        if line == 0 || character == 0 {
            return Err(LspRuntimeError::InvalidQuery(
                "line and character are 1-based and must be positive".to_string(),
            ));
        }
        Some(serde_json::json!({
            "line": line - 1,
            "character": character - 1,
        }))
    } else {
        None
    };
    let text_document = serde_json::json!({ "uri": uri });
    let output = match query.operation {
        LspQueryOperation::GoToDefinition => (
            "textDocument/definition",
            serde_json::json!({ "textDocument": text_document, "position": position }),
        ),
        LspQueryOperation::FindReferences => (
            "textDocument/references",
            serde_json::json!({
                "textDocument": text_document,
                "position": position,
                "context": { "includeDeclaration": true },
            }),
        ),
        LspQueryOperation::Hover => (
            "textDocument/hover",
            serde_json::json!({ "textDocument": text_document, "position": position }),
        ),
        LspQueryOperation::DocumentSymbol => (
            "textDocument/documentSymbol",
            serde_json::json!({ "textDocument": text_document }),
        ),
        LspQueryOperation::WorkspaceSymbol => (
            "workspace/symbol",
            serde_json::json!({ "query": query.query.clone().unwrap_or_default() }),
        ),
        LspQueryOperation::GoToImplementation => (
            "textDocument/implementation",
            serde_json::json!({ "textDocument": text_document, "position": position }),
        ),
        LspQueryOperation::PrepareCallHierarchy
        | LspQueryOperation::IncomingCalls
        | LspQueryOperation::OutgoingCalls => (
            "textDocument/prepareCallHierarchy",
            serde_json::json!({ "textDocument": text_document, "position": position }),
        ),
        LspQueryOperation::Diagnostics => unreachable!("diagnostics does not call LSP server"),
    };
    Ok((output.0.to_string(), output.1))
}

fn rust_analyzer_definition(workspace_root: &Path, command: &str) -> LspServerDefinition {
    LspServerDefinition {
        id: RUST_ANALYZER_ID.to_string(),
        display_name: "rust-analyzer".to_string(),
        command: command.to_string(),
        args: Vec::new(),
        extensions: vec![".rs".to_string()],
        language_ids: vec!["rust".to_string()],
        workspace_root: workspace_root.to_path_buf(),
    }
}

fn extensions_for_language(definition: &LspServerDefinition, language_id: &str) -> Vec<String> {
    if definition.language_ids.len() == definition.extensions.len() {
        definition
            .language_ids
            .iter()
            .zip(definition.extensions.iter())
            .filter(|(candidate, _)| candidate.as_str() == language_id)
            .map(|(_, extension)| extension.clone())
            .collect()
    } else if definition
        .language_ids
        .iter()
        .any(|candidate| candidate == language_id)
    {
        definition.extensions.clone()
    } else {
        Vec::new()
    }
}

#[derive(Debug, Clone)]
enum ProbeError {
    MissingCommand,
    MissingRustupComponent,
    Failed(String),
}

async fn probe_rust_analyzer(command: &str) -> Result<String, ProbeError> {
    if !is_builtin_rust_analyzer_command(command) {
        return probe_command(command).await;
    }
    match probe_command(command).await {
        Ok(version) => Ok(version),
        Err(ProbeError::MissingCommand) => {
            if !rustup_is_available().await {
                return Err(ProbeError::MissingCommand);
            }
            install_rust_analyzer_component().await?;
            probe_command(command).await
        }
        Err(ProbeError::MissingRustupComponent) => {
            if !rustup_is_available().await {
                return Err(ProbeError::Failed(
                    "rust-analyzer component is missing, but rustup was not found on PATH"
                        .to_string(),
                ));
            }
            install_rust_analyzer_component().await?;
            probe_command(command).await
        }
        Err(error) => Err(error),
    }
}

async fn probe_command(command: &str) -> Result<String, ProbeError> {
    let output = run_command_capture(
        command,
        &["--version"],
        PROBE_TIMEOUT,
        "rust-analyzer --version timed out",
    )
    .await?;
    if output.status.success() {
        let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
        return Ok(if version.is_empty() {
            "rust-analyzer is available".to_string()
        } else {
            version
        });
    }
    let message = command_failure_message(output.status, &output.stdout, &output.stderr);
    if is_rustup_missing_component_error(&message) {
        return Err(ProbeError::MissingRustupComponent);
    }
    Err(ProbeError::Failed(message))
}

async fn rustup_is_available() -> bool {
    run_command_capture(
        "rustup",
        &["--version"],
        PROBE_TIMEOUT,
        "rustup --version timed out",
    )
    .await
    .map(|output| output.status.success())
    .unwrap_or(false)
}

async fn install_rust_analyzer_component() -> Result<(), ProbeError> {
    let output = run_command_capture(
        "rustup",
        &["component", "add", "rust-analyzer"],
        RUSTUP_TIMEOUT,
        "rustup component add rust-analyzer timed out",
    )
    .await
    .map_err(|error| match error {
        ProbeError::MissingCommand => ProbeError::Failed(
            "rust-analyzer component is missing, but rustup was not found on PATH".to_string(),
        ),
        other => other,
    })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(ProbeError::Failed(format!(
            "failed to install rust-analyzer component with `rustup component add rust-analyzer`: {}",
            command_failure_message(output.status, &output.stdout, &output.stderr)
        )))
    }
}

struct CapturedCommandOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

async fn run_command_capture(
    command: &str,
    args: &[&str],
    timeout: Duration,
    timeout_message: &str,
) -> Result<CapturedCommandOutput, ProbeError> {
    let mut command_process = Command::new(command);
    command_process
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_background_command(&mut command_process);
    let mut child = command_process.spawn().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ProbeError::MissingCommand
        } else {
            ProbeError::Failed(error.to_string())
        }
    })?;
    let pid = child.id();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_task = tokio::spawn(read_child_output(stdout));
    let stderr_task = tokio::spawn(read_child_output(stderr));
    let status = match tokio::time::timeout(timeout, child.wait()).await {
        Ok(Ok(status)) => status,
        Ok(Err(error)) => return Err(ProbeError::Failed(error.to_string())),
        Err(_) => {
            terminate_process_tree(pid).await;
            let _ = child.start_kill();
            let _ = child.wait().await;
            return Err(ProbeError::Failed(timeout_message.to_string()));
        }
    };
    let stdout = stdout_task.await.unwrap_or_default();
    let stderr = stderr_task.await.unwrap_or_default();
    Ok(CapturedCommandOutput {
        status,
        stdout,
        stderr,
    })
}

fn command_failure_message(status: ExitStatus, stdout: &[u8], stderr: &[u8]) -> String {
    let stderr = String::from_utf8_lossy(stderr).trim().to_string();
    if !stderr.is_empty() {
        stderr
    } else {
        let stdout = String::from_utf8_lossy(stdout).trim().to_string();
        if stdout.is_empty() {
            format!("command exited with {status}")
        } else {
            stdout
        }
    }
}

fn missing_rust_analyzer_message() -> String {
    "rust-analyzer command not found; if you use rustup, ensure it is on PATH so Pure Studio can run `rustup component add rust-analyzer` automatically".to_string()
}

fn is_builtin_rust_analyzer_command(command: &str) -> bool {
    let path = Path::new(command);
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| stem.eq_ignore_ascii_case(RUST_ANALYZER_COMMAND))
        .unwrap_or_else(|| command.eq_ignore_ascii_case(RUST_ANALYZER_COMMAND))
}

fn is_rustup_missing_component_error(stderr: &str) -> bool {
    let stderr = stderr.to_ascii_lowercase();
    stderr.contains("unknown binary")
        && (stderr.contains("rust-analyzer") || stderr.contains("rust_analyzer"))
}

async fn read_child_output(stream: Option<impl tokio::io::AsyncRead + Unpin>) -> Vec<u8> {
    let Some(mut stream) = stream else {
        return Vec::new();
    };
    let mut output = Vec::new();
    let _ = stream.read_to_end(&mut output).await;
    output
}

fn diagnostic_counts(diagnostics: &HashMap<String, Vec<LspDiagnostic>>) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for values in diagnostics.values() {
        for diagnostic in values {
            *counts.entry(diagnostic.server_id.clone()).or_insert(0) += 1;
        }
    }
    counts
}

fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use pretty_assertions::assert_eq;

    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("pure-lsp-{name}-{stamp}"))
    }

    async fn register_available_rust(registry: &LspRuntimeRegistry, workspace_root: &Path) {
        let definition = rust_analyzer_definition(workspace_root, RUST_ANALYZER_COMMAND);
        let mut state = registry.state.lock().await;
        state.workspace_root = Some(workspace_root.to_path_buf());
        state.servers.insert(
            RUST_ANALYZER_ID.to_string(),
            LspRuntimeServerState::new(
                definition,
                LspAvailabilityKind::Available,
                Some("rust-analyzer test".to_string()),
                Some(1),
            ),
        );
    }

    #[tokio::test]
    async fn missing_rust_analyzer_command_records_snapshot() {
        let dir = temp_dir("missing-command");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname='x'\nversion='0.1.0'\n",
        )
        .unwrap();
        let registry = LspRuntimeRegistry::new();

        registry
            .reconcile_workspace_with_command(&dir, "definitely-not-rust-analyzer-pure-test")
            .await;
        let snapshots = registry.snapshots().await;

        assert_eq!(snapshots.len(), 1);
        assert_eq!(
            snapshots[0].availability_kind,
            LspAvailabilityKind::MissingCommand
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rustup_unknown_binary_is_missing_component() {
        assert!(is_rustup_missing_component_error(
            "error: Unknown binary 'rust-analyzer.exe' in official toolchain 'stable-x86_64-pc-windows-msvc'."
        ));
        assert!(!is_rustup_missing_component_error(
            "error: Unknown binary 'cargo-miri.exe' in official toolchain"
        ));
    }

    #[tokio::test]
    #[ignore = "requires rust-analyzer component and starts the language server"]
    async fn live_rust_analyzer_document_symbol_query() {
        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root")
            .to_path_buf();
        let registry = LspRuntimeRegistry::new();

        registry.reconcile_workspace(&workspace_root).await;
        let snapshots = registry.snapshots().await;

        assert_eq!(snapshots.len(), 1);
        assert_eq!(
            snapshots[0].availability_kind,
            LspAvailabilityKind::Available,
            "{}",
            snapshots[0].availability_message.as_deref().unwrap_or("")
        );

        let result = registry
            .query(LspQuery {
                operation: LspQueryOperation::DocumentSymbol,
                file_path: Some(workspace_root.join("code/pl-lsp/src/registry.rs")),
                line: None,
                character: None,
                query: None,
                max_results: None,
                language_id: None,
            })
            .await
            .expect("document symbol query");

        assert!(result.success);
        assert_eq!(result.server_id.as_deref(), Some(RUST_ANALYZER_ID));
        assert!(
            result.result.contains("LspRuntimeRegistry"),
            "{}",
            result.result
        );
        registry.shutdown().await;
    }

    #[tokio::test]
    async fn available_languages_returns_empty_when_no_servers() {
        let registry = LspRuntimeRegistry::new();
        let languages = registry.available_languages().await;
        assert!(languages.is_empty());
    }

    #[tokio::test]
    async fn available_languages_returns_empty_when_server_not_available() {
        let dir = temp_dir("available-languages");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname='x'\nversion='0.1.0'\n",
        )
        .unwrap();
        let registry = LspRuntimeRegistry::new();
        registry
            .reconcile_workspace_with_command(&dir, "definitely-not-rust-analyzer-pure-test")
            .await;

        let languages = registry.available_languages().await;
        assert!(languages.is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn available_languages_returns_available_server_languages() {
        let dir = temp_dir("available-languages-rust");
        fs::create_dir_all(&dir).unwrap();
        let registry = LspRuntimeRegistry::new();
        register_available_rust(&registry, &dir).await;

        let languages = registry.available_languages().await;

        assert_eq!(
            languages,
            vec![LanguageToolInfo {
                language_id: "rust".to_string(),
                server_id: RUST_ANALYZER_ID.to_string(),
                display_name: "rust-analyzer".to_string(),
                extensions: vec![".rs".to_string()],
            }]
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn server_id_for_query_prefers_language_id() {
        let dir = temp_dir("language-route");
        fs::create_dir_all(&dir).unwrap();
        let registry = LspRuntimeRegistry::new();
        register_available_rust(&registry, &dir).await;
        let query = LspQuery {
            operation: LspQueryOperation::WorkspaceSymbol,
            file_path: None,
            line: None,
            character: None,
            query: Some("LspRuntimeRegistry".to_string()),
            max_results: None,
            language_id: Some("rust".to_string()),
        };

        let server_id = registry.server_id_for_query(&query).await.unwrap();

        assert_eq!(server_id, RUST_ANALYZER_ID);
        fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn diagnostics_query_with_language_id_reports_target_server() {
        let dir = temp_dir("diagnostics-language-route");
        fs::create_dir_all(&dir).unwrap();
        let registry = LspRuntimeRegistry::new();
        register_available_rust(&registry, &dir).await;

        let result = registry
            .query(LspQuery {
                operation: LspQueryOperation::Diagnostics,
                file_path: None,
                line: None,
                character: None,
                query: None,
                max_results: None,
                language_id: Some("rust".to_string()),
            })
            .await
            .unwrap();

        assert!(result.success);
        assert_eq!(result.server_id.as_deref(), Some(RUST_ANALYZER_ID));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn position_query_uses_one_based_input() {
        let query = LspQuery {
            operation: LspQueryOperation::Hover,
            file_path: Some(PathBuf::from("src/lib.rs")),
            line: Some(7),
            character: Some(3),
            query: None,
            max_results: None,
            language_id: None,
        };

        let (_, params) = method_and_params(&query).unwrap();

        assert_eq!(params["position"]["line"], serde_json::json!(6));
        assert_eq!(params["position"]["character"], serde_json::json!(2));
    }
}
