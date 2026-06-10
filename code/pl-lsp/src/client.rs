use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::time::Duration;

use lsp_types::{
    DiagnosticSeverity, DidChangeWatchedFilesParams, FileChangeType, FileEvent, NumberOrString,
    ProgressParams, ProgressParamsValue, ProgressToken, PublishDiagnosticsParams, Uri,
    WorkDoneProgress, WorkDoneProgressCreateParams,
};
use serde_json::Value;
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{Mutex, oneshot};

use crate::framing::{encode_message, read_message};
use crate::process::{configure_background_command, terminate_process_tree};
use crate::types::{
    LspActivityKind, LspDiagnostic, LspPosition, LspRange, LspResult, LspRuntimeError,
};
use crate::uri::{file_uri_to_path, normalize_separators};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const SHUTDOWN_WAIT_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_FILE_SIZE_BYTES: u64 = 10_000_000;
const RUST_ANALYZER_ID: &str = "rust-analyzer";

type PendingResponseSender = oneshot::Sender<LspResult<Value>>;
type PendingRequests = Arc<Mutex<HashMap<i64, PendingResponseSender>>>;

#[derive(Debug, Clone)]
pub(crate) struct LspServerDefinition {
    pub id: String,
    pub display_name: String,
    pub command: String,
    pub args: Vec<String>,
    pub extensions: Vec<String>,
    pub language_ids: Vec<String>,
    pub workspace_root: PathBuf,
}

impl LspServerDefinition {
    pub fn language_for_path(&self, path: &Path) -> Option<&str> {
        let extension = path.extension()?.to_string_lossy().to_ascii_lowercase();
        let extension = format!(".{extension}");
        self.extensions
            .iter()
            .position(|candidate| candidate == &extension)
            .and_then(|index| self.language_ids.get(index))
            .map(String::as_str)
    }
}

#[derive(Clone)]
pub(crate) struct DiagnosticSink {
    server_id: String,
    workspace_root: PathBuf,
    diagnostics: Arc<Mutex<HashMap<String, Vec<LspDiagnostic>>>>,
    updates: tokio::sync::broadcast::Sender<()>,
}

impl DiagnosticSink {
    pub fn new(
        server_id: String,
        workspace_root: PathBuf,
        diagnostics: Arc<Mutex<HashMap<String, Vec<LspDiagnostic>>>>,
        updates: tokio::sync::broadcast::Sender<()>,
    ) -> Self {
        Self {
            server_id,
            workspace_root,
            diagnostics,
            updates,
        }
    }

    async fn publish(&self, params: PublishDiagnosticsParams) {
        let received_at = unix_seconds();
        let path = file_uri_to_path(params.uri.as_str());
        let display_path = path
            .strip_prefix(&self.workspace_root)
            .map(normalize_separators)
            .unwrap_or_else(|_| normalize_separators(&path));
        let diagnostics = params
            .diagnostics
            .into_iter()
            .map(|diagnostic| LspDiagnostic {
                server_id: self.server_id.clone(),
                uri: params.uri.as_str().to_string(),
                path: display_path.clone(),
                range: LspRange {
                    start: LspPosition {
                        line: diagnostic.range.start.line,
                        character: diagnostic.range.start.character,
                    },
                    end: LspPosition {
                        line: diagnostic.range.end.line,
                        character: diagnostic.range.end.character,
                    },
                },
                severity: diagnostic.severity.map(diagnostic_severity),
                message: diagnostic.message,
                source: diagnostic.source,
                code: diagnostic.code.map(number_or_string),
                received_at,
            })
            .collect::<Vec<_>>();
        self.diagnostics.lock().await.insert(
            format!("{}:{}", self.server_id, params.uri.as_str()),
            diagnostics,
        );
        let _ = self.updates.send(());
    }
}

pub(crate) struct LspClient {
    definition: LspServerDefinition,
    child: Mutex<Option<Child>>,
    stdin: Arc<Mutex<Option<ChildStdin>>>,
    pending: PendingRequests,
    opened_files: Mutex<HashMap<PathBuf, OpenDocument>>,
    next_id: AtomicI64,
    initialized: AtomicBool,
    start_lock: Mutex<()>,
    diagnostics: DiagnosticSink,
    status: Arc<Mutex<LspClientStatus>>,
}

impl std::fmt::Debug for LspClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LspClient")
            .field("server_id", &self.definition.id)
            .field("workspace_root", &self.definition.workspace_root)
            .field("initialized", &self.initialized.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
struct OpenDocument {
    uri: String,
    version: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LspClientRuntimeStatus {
    pub activity_kind: LspActivityKind,
    pub activity_title: Option<String>,
    pub activity_message: Option<String>,
    pub activity_percentage: Option<u32>,
    pub last_error: Option<String>,
    pub last_error_at: Option<i64>,
}

impl Default for LspClientRuntimeStatus {
    fn default() -> Self {
        Self {
            activity_kind: LspActivityKind::Idle,
            activity_title: None,
            activity_message: None,
            activity_percentage: None,
            last_error: None,
            last_error_at: None,
        }
    }
}

#[derive(Debug, Clone)]
struct LspProgressEntry {
    activity_kind: LspActivityKind,
    title: String,
    message: Option<String>,
    percentage: Option<u32>,
    sequence: u64,
}

#[derive(Debug, Default)]
struct LspClientStatus {
    registered_progress_tokens: HashSet<ProgressToken>,
    progress: HashMap<ProgressToken, LspProgressEntry>,
    next_progress_sequence: u64,
    last_error: Option<String>,
    last_error_at: Option<i64>,
}

impl LspClientStatus {
    fn register_progress_token(&mut self, token: ProgressToken) -> bool {
        self.registered_progress_tokens.insert(token)
    }

    fn apply_progress(&mut self, params: ProgressParams) -> bool {
        if !self.registered_progress_tokens.contains(&params.token) {
            return false;
        }
        match params.value {
            ProgressParamsValue::WorkDone(WorkDoneProgress::Begin(progress)) => {
                self.next_progress_sequence += 1;
                let activity_kind =
                    activity_kind_for_progress(&progress.title, progress.message.as_deref());
                self.progress.insert(
                    params.token,
                    LspProgressEntry {
                        activity_kind,
                        title: progress.title,
                        message: progress.message,
                        percentage: progress.percentage,
                        sequence: self.next_progress_sequence,
                    },
                );
                true
            }
            ProgressParamsValue::WorkDone(WorkDoneProgress::Report(progress)) => {
                let Some(entry) = self.progress.get_mut(&params.token) else {
                    return false;
                };
                self.next_progress_sequence += 1;
                entry.sequence = self.next_progress_sequence;
                if let Some(message) = progress.message {
                    entry.message = Some(message);
                }
                if let Some(percentage) = progress.percentage {
                    entry.percentage = Some(percentage);
                }
                entry.activity_kind =
                    activity_kind_for_progress(&entry.title, entry.message.as_deref());
                true
            }
            ProgressParamsValue::WorkDone(WorkDoneProgress::End(_)) => {
                self.registered_progress_tokens.remove(&params.token);
                self.progress.remove(&params.token).is_some()
            }
        }
    }

    fn clear_progress(&mut self) -> bool {
        let changed = !self.registered_progress_tokens.is_empty() || !self.progress.is_empty();
        self.registered_progress_tokens.clear();
        self.progress.clear();
        changed
    }

    fn record_error(&mut self, message: String) -> bool {
        self.last_error = Some(message);
        self.last_error_at = Some(unix_seconds());
        true
    }

    fn runtime_status(&self) -> LspClientRuntimeStatus {
        let Some(entry) = self.progress.values().max_by_key(|entry| entry.sequence) else {
            return LspClientRuntimeStatus {
                last_error: self.last_error.clone(),
                last_error_at: self.last_error_at,
                ..LspClientRuntimeStatus::default()
            };
        };
        LspClientRuntimeStatus {
            activity_kind: entry.activity_kind,
            activity_title: Some(entry.title.clone()),
            activity_message: entry.message.clone(),
            activity_percentage: entry.percentage,
            last_error: self.last_error.clone(),
            last_error_at: self.last_error_at,
        }
    }
}

impl LspClient {
    pub fn new(definition: LspServerDefinition, diagnostics: DiagnosticSink) -> Self {
        Self {
            definition,
            child: Mutex::new(None),
            stdin: Arc::new(Mutex::new(None)),
            pending: Arc::new(Mutex::new(HashMap::new())),
            opened_files: Mutex::new(HashMap::new()),
            next_id: AtomicI64::new(1),
            initialized: AtomicBool::new(false),
            start_lock: Mutex::new(()),
            diagnostics,
            status: Arc::new(Mutex::new(LspClientStatus::default())),
        }
    }

    pub async fn request(&self, method: &str, params: Value) -> LspResult<Value> {
        self.ensure_started().await?;
        let result = request_raw(
            &self.stdin,
            &self.pending,
            &self.next_id,
            method,
            params,
            REQUEST_TIMEOUT,
        )
        .await;
        if let Err(error) = &result
            && !is_content_modified_error(error)
        {
            self.record_last_error(error.to_string()).await;
        }
        result
    }

    pub async fn notify(&self, method: &str, params: Value) -> LspResult<()> {
        self.ensure_started().await?;
        notify_raw(&self.stdin, method, params).await
    }

    pub async fn open_or_change_file(&self, path: &Path) -> LspResult<()> {
        self.ensure_started().await?;
        let metadata = tokio::fs::metadata(path).await?;
        if metadata.len() > MAX_FILE_SIZE_BYTES {
            return Err(LspRuntimeError::Unavailable(format!(
                "file too large for LSP analysis: {}",
                path.display()
            )));
        }
        let text = tokio::fs::read_to_string(path).await?;
        let language_id = self
            .definition
            .language_for_path(path)
            .unwrap_or("plaintext")
            .to_string();
        let uri = crate::uri::path_to_file_uri(path);
        let mut opened = self.opened_files.lock().await;
        if let Some(document) = opened.get_mut(path) {
            document.version += 1;
            self.notify(
                "textDocument/didChange",
                serde_json::json!({
                    "textDocument": {
                        "uri": document.uri,
                        "version": document.version,
                    },
                    "contentChanges": [{ "text": text }],
                }),
            )
            .await?;
        } else {
            self.notify(
                "textDocument/didOpen",
                serde_json::json!({
                    "textDocument": {
                        "uri": uri,
                        "languageId": language_id,
                        "version": 1,
                        "text": text,
                    },
                }),
            )
            .await?;
            opened.insert(path.to_path_buf(), OpenDocument { uri, version: 1 });
        }
        Ok(())
    }

    pub async fn save_file(&self, path: &Path) -> LspResult<()> {
        if let Some(document) = self.opened_files.lock().await.get(path).cloned() {
            self.notify(
                "textDocument/didSave",
                serde_json::json!({
                    "textDocument": { "uri": document.uri },
                }),
            )
            .await?;
        }
        Ok(())
    }

    pub async fn close_file(&self, path: &Path) -> LspResult<()> {
        let document = self.opened_files.lock().await.remove(path);
        if let Some(document) = document {
            self.notify(
                "textDocument/didClose",
                serde_json::json!({
                    "textDocument": { "uri": document.uri },
                }),
            )
            .await?;
        }
        Ok(())
    }

    pub async fn notify_watched_file_changed(&self, path: &Path) -> LspResult<()> {
        self.notify_watched_file_event(path, FileChangeType::CHANGED)
            .await
    }

    pub async fn notify_watched_file_deleted(&self, path: &Path) -> LspResult<()> {
        self.notify_watched_file_event(path, FileChangeType::DELETED)
            .await
    }

    pub async fn shutdown(&self) {
        if self.initialized.swap(false, Ordering::Relaxed) {
            let _ = request_raw(
                &self.stdin,
                &self.pending,
                &self.next_id,
                "shutdown",
                Value::Null,
                Duration::from_secs(3),
            )
            .await;
            let _ = notify_raw(&self.stdin, "exit", Value::Null).await;
        }
        self.stdin.lock().await.take();
        if let Some(mut child) = self.child.lock().await.take() {
            let pid = child.id();
            if tokio::time::timeout(SHUTDOWN_WAIT_TIMEOUT, child.wait())
                .await
                .is_err()
            {
                terminate_process_tree(pid).await;
                let _ = child.start_kill();
                let _ = child.wait().await;
            }
        }
        self.opened_files.lock().await.clear();
        self.pending.lock().await.clear();
        self.clear_progress().await;
    }

    pub async fn runtime_status(&self) -> LspClientRuntimeStatus {
        self.status.lock().await.runtime_status()
    }

    async fn notify_watched_file_event(&self, path: &Path, typ: FileChangeType) -> LspResult<()> {
        if !self.initialized.load(Ordering::Relaxed) {
            return Ok(());
        }
        notify_raw(
            &self.stdin,
            "workspace/didChangeWatchedFiles",
            watched_file_event_params(path, typ)?,
        )
        .await
    }

    async fn ensure_started(&self) -> LspResult<()> {
        if self.initialized.load(Ordering::Relaxed) {
            return Ok(());
        }
        let _guard = self.start_lock.lock().await;
        if self.initialized.load(Ordering::Relaxed) {
            return Ok(());
        }

        let mut command = Command::new(&self.definition.command);
        command
            .args(&self.definition.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .current_dir(&self.definition.workspace_root);
        configure_background_command(&mut command);
        let mut child = command.spawn().map_err(|error| {
            LspRuntimeError::Unavailable(format!(
                "failed to start {}: {error}",
                self.definition.command
            ))
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            LspRuntimeError::Unavailable("LSP server stdout unavailable".to_string())
        })?;
        let stdin = child.stdin.take().ok_or_else(|| {
            LspRuntimeError::Unavailable("LSP server stdin unavailable".to_string())
        })?;
        let stderr = child.stderr.take();
        self.stdin.lock().await.replace(stdin);
        self.spawn_reader(stdout);
        if let Some(stderr) = stderr {
            let status = self.status.clone();
            let updates = self.diagnostics.updates.clone();
            tokio::spawn(async move {
                let mut reader = BufReader::new(stderr);
                loop {
                    let mut line = String::new();
                    match tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line).await {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            let line = line.trim();
                            if !line.is_empty() {
                                if is_error_stderr_line(line)
                                    && record_last_error_status(&status, line.to_string()).await
                                {
                                    let _ = updates.send(());
                                }
                                eprintln!("[pl-lsp] {line}");
                            }
                        }
                    }
                }
            });
        }
        self.child.lock().await.replace(child);

        let initialize = request_raw(
            &self.stdin,
            &self.pending,
            &self.next_id,
            "initialize",
            initialize_params(&self.definition),
            STARTUP_TIMEOUT,
        )
        .await;
        if let Err(error) = initialize {
            self.record_last_error(error.to_string()).await;
            self.shutdown().await;
            return Err(error);
        }
        notify_raw(&self.stdin, "initialized", serde_json::json!({})).await?;
        self.initialized.store(true, Ordering::Relaxed);
        Ok(())
    }

    fn spawn_reader(&self, stdout: tokio::process::ChildStdout) {
        let pending = self.pending.clone();
        let stdin = self.stdin.clone();
        let diagnostics = self.diagnostics.clone();
        let status = self.status.clone();
        let updates = self.diagnostics.updates.clone();
        let server_id = self.definition.id.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            loop {
                let message = match read_message(&mut reader).await {
                    Ok(Some(message)) => message,
                    Ok(None) => break,
                    Err(error) => {
                        fail_pending(&pending, error).await;
                        break;
                    }
                };
                let value = match serde_json::from_slice::<Value>(&message) {
                    Ok(value) => value,
                    Err(error) => {
                        fail_pending(&pending, error.into()).await;
                        continue;
                    }
                };
                if let Some(method) = value.get("method").and_then(Value::as_str) {
                    if let Some(id) = response_id(&value) {
                        let _ = respond_to_server_request(
                            &stdin,
                            id,
                            method,
                            value.get("params"),
                            &server_id,
                            &status,
                            &updates,
                        )
                        .await;
                    } else if method == "$/progress"
                        && let Some(params) = value.get("params")
                        && let Ok(params) = serde_json::from_value::<ProgressParams>(params.clone())
                    {
                        if apply_progress_status(&status, params).await {
                            let _ = updates.send(());
                        }
                    } else if method == "textDocument/publishDiagnostics"
                        && let Some(params) = value.get("params")
                        && let Ok(params) =
                            serde_json::from_value::<PublishDiagnosticsParams>(params.clone())
                    {
                        diagnostics.publish(params).await;
                    }
                    continue;
                }
                if let Some(id) = response_id(&value) {
                    let result = response_result(value);
                    if let Err(error) = &result
                        && !is_content_modified_error(error)
                        && record_last_error_status(&status, error.to_string()).await
                    {
                        let _ = updates.send(());
                    }
                    if let Some(sender) = pending.lock().await.remove(&id) {
                        let _ = sender.send(result);
                    }
                }
            }
            if clear_progress_status(&status).await {
                let _ = updates.send(());
            }
            fail_pending(
                &pending,
                LspRuntimeError::Unavailable(format!("{server_id} connection closed")),
            )
            .await;
        });
    }

    async fn record_last_error(&self, message: String) {
        if record_last_error_status(&self.status, message).await {
            let _ = self.diagnostics.updates.send(());
        }
    }

    async fn clear_progress(&self) {
        if clear_progress_status(&self.status).await {
            let _ = self.diagnostics.updates.send(());
        }
    }
}

async fn request_raw(
    stdin: &Arc<Mutex<Option<ChildStdin>>>,
    pending: &PendingRequests,
    next_id: &AtomicI64,
    method: &str,
    params: Value,
    timeout: Duration,
) -> LspResult<Value> {
    let id = next_id.fetch_add(1, Ordering::Relaxed);
    let (sender, receiver) = oneshot::channel();
    pending.lock().await.insert(id, sender);
    let write = write_message(
        stdin,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }),
    )
    .await;
    if let Err(error) = write {
        pending.lock().await.remove(&id);
        return Err(error);
    }
    match tokio::time::timeout(timeout, receiver).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => Err(LspRuntimeError::Unavailable(format!(
            "LSP request channel closed for {method}"
        ))),
        Err(_) => {
            pending.lock().await.remove(&id);
            Err(LspRuntimeError::Timeout(method.to_string()))
        }
    }
}

async fn notify_raw(
    stdin: &Arc<Mutex<Option<ChildStdin>>>,
    method: &str,
    params: Value,
) -> LspResult<()> {
    write_message(
        stdin,
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }),
    )
    .await
}

async fn write_message(stdin: &Arc<Mutex<Option<ChildStdin>>>, message: Value) -> LspResult<()> {
    let bytes = encode_message(&message)?;
    let mut guard = stdin.lock().await;
    let stdin = guard
        .as_mut()
        .ok_or_else(|| LspRuntimeError::Unavailable("LSP stdin unavailable".to_string()))?;
    stdin.write_all(&bytes).await?;
    stdin.flush().await?;
    Ok(())
}

async fn respond_to_server_request(
    stdin: &Arc<Mutex<Option<ChildStdin>>>,
    id: i64,
    method: &str,
    params: Option<&Value>,
    server_id: &str,
    status: &Arc<Mutex<LspClientStatus>>,
    updates: &tokio::sync::broadcast::Sender<()>,
) -> LspResult<()> {
    let result = match method {
        "workspace/configuration" => workspace_configuration_response(params, server_id),
        "window/workDoneProgress/create" => {
            if let Some(params) = params
                && let Ok(params) =
                    serde_json::from_value::<WorkDoneProgressCreateParams>(params.clone())
                && register_progress_token_status(status, params.token).await
            {
                let _ = updates.send(());
            }
            Value::Null
        }
        "client/registerCapability" | "client/unregisterCapability" => Value::Null,
        _ => Value::Null,
    };
    write_message(
        stdin,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        }),
    )
    .await
}

async fn register_progress_token_status(
    status: &Arc<Mutex<LspClientStatus>>,
    token: ProgressToken,
) -> bool {
    status.lock().await.register_progress_token(token)
}

async fn apply_progress_status(
    status: &Arc<Mutex<LspClientStatus>>,
    params: ProgressParams,
) -> bool {
    status.lock().await.apply_progress(params)
}

async fn clear_progress_status(status: &Arc<Mutex<LspClientStatus>>) -> bool {
    status.lock().await.clear_progress()
}

async fn record_last_error_status(status: &Arc<Mutex<LspClientStatus>>, message: String) -> bool {
    status.lock().await.record_error(message)
}

fn initialize_params(definition: &LspServerDefinition) -> Value {
    let workspace_uri = crate::uri::path_to_file_uri(&definition.workspace_root);
    let workspace_name = definition
        .workspace_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("workspace");
    serde_json::json!({
        "processId": null,
        "rootPath": definition.workspace_root,
        "rootUri": workspace_uri,
        "workspaceFolders": [{
            "uri": workspace_uri,
            "name": workspace_name,
        }],
        "capabilities": {
            "window": {
                "workDoneProgress": true,
            },
            "workspace": {
                "configuration": true,
                "didChangeWatchedFiles": {
                    "dynamicRegistration": true,
                },
                "workspaceFolders": false,
            },
            "textDocument": {
                "synchronization": {
                    "dynamicRegistration": false,
                    "willSave": false,
                    "willSaveWaitUntil": false,
                    "didSave": true,
                },
                "publishDiagnostics": {
                    "relatedInformation": true,
                    "tagSupport": { "valueSet": [1, 2] },
                    "versionSupport": false,
                    "codeDescriptionSupport": true,
                    "dataSupport": false,
                },
                "hover": {
                    "dynamicRegistration": false,
                    "contentFormat": ["markdown", "plaintext"],
                },
                "definition": {
                    "dynamicRegistration": false,
                    "linkSupport": true,
                },
                "implementation": {
                    "dynamicRegistration": false,
                    "linkSupport": true,
                },
                "references": { "dynamicRegistration": false },
                "documentSymbol": {
                    "dynamicRegistration": false,
                    "hierarchicalDocumentSymbolSupport": true,
                },
                "callHierarchy": { "dynamicRegistration": false },
            },
            "general": {
                "positionEncodings": ["utf-16"],
            },
        },
        "initializationOptions": initialization_options(definition),
    })
}

fn initialization_options(definition: &LspServerDefinition) -> Value {
    if definition.id == RUST_ANALYZER_ID {
        rust_analyzer_settings()
    } else {
        Value::Null
    }
}

fn workspace_configuration_response(params: Option<&Value>, server_id: &str) -> Value {
    let Some(items) = params
        .and_then(|params| params.get("items"))
        .and_then(Value::as_array)
    else {
        return serde_json::json!([]);
    };
    Value::Array(
        items
            .iter()
            .map(|item| {
                let section = item.get("section").and_then(Value::as_str);
                configuration_value_for_section(server_id, section)
            })
            .collect(),
    )
}

fn configuration_value_for_section(server_id: &str, section: Option<&str>) -> Value {
    if server_id != RUST_ANALYZER_ID {
        return Value::Null;
    }
    match section {
        Some("rust-analyzer") | None => rust_analyzer_settings(),
        Some("rust-analyzer.files") => serde_json::json!({ "watcher": "client" }),
        Some("rust-analyzer.files.watcher") => serde_json::json!("client"),
        Some(_) => Value::Null,
    }
}

fn rust_analyzer_settings() -> Value {
    serde_json::json!({
        "files": {
            "watcher": "client",
        },
    })
}

fn activity_kind_for_progress(title: &str, message: Option<&str>) -> LspActivityKind {
    let text = format!("{} {}", title, message.unwrap_or_default()).to_ascii_lowercase();
    if text.contains("index")
        || text.contains("fetching")
        || text.contains("crategraph")
        || text.contains("crate graph")
        || text.contains("roots scanned")
        || text.contains("cargo metadata")
        || text.contains("compile-time-deps")
        || text.contains("discovering sysroot")
        || text.contains("querying project metadata")
    {
        LspActivityKind::Indexing
    } else {
        LspActivityKind::Busy
    }
}

fn is_error_stderr_line(line: &str) -> bool {
    let line = line.to_ascii_lowercase();
    line.contains("warn") || line.contains("error")
}

fn watched_file_event_params(path: &Path, typ: FileChangeType) -> LspResult<Value> {
    let uri = Uri::from_str(&crate::uri::path_to_file_uri(path)).map_err(|error| {
        LspRuntimeError::InvalidQuery(format!("invalid file URI for {}: {error}", path.display()))
    })?;
    let params = DidChangeWatchedFilesParams {
        changes: vec![FileEvent::new(uri, typ)],
    };
    Ok(serde_json::to_value(params)?)
}

fn response_id(value: &Value) -> Option<i64> {
    value
        .get("id")
        .and_then(|id| id.as_i64().or_else(|| id.as_u64().map(|id| id as i64)))
}

fn response_result(value: Value) -> LspResult<Value> {
    if let Some(error) = value.get("error") {
        let code = error
            .get("code")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("LSP request failed")
            .to_string();
        return Err(LspRuntimeError::Server { code, message });
    }
    Ok(value.get("result").cloned().unwrap_or(Value::Null))
}

async fn fail_pending(pending: &PendingRequests, error: LspRuntimeError) {
    let mut pending = pending.lock().await;
    for (_, sender) in pending.drain() {
        let _ = sender.send(Err(LspRuntimeError::Unavailable(error.to_string())));
    }
}

fn diagnostic_severity(severity: DiagnosticSeverity) -> u32 {
    if severity == DiagnosticSeverity::ERROR {
        1
    } else if severity == DiagnosticSeverity::WARNING {
        2
    } else if severity == DiagnosticSeverity::INFORMATION {
        3
    } else if severity == DiagnosticSeverity::HINT {
        4
    } else {
        0
    }
}

fn number_or_string(value: NumberOrString) -> String {
    match value {
        NumberOrString::Number(number) => number.to_string(),
        NumberOrString::String(text) => text,
    }
}

pub(crate) fn is_content_modified_error(error: &LspRuntimeError) -> bool {
    matches!(error, LspRuntimeError::Server { code: -32801, .. })
}

pub(crate) async fn with_content_modified_retries<F, Fut>(mut operation: F) -> LspResult<Value>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = LspResult<Value>>,
{
    let mut delay = Duration::from_millis(500);
    for attempt in 0..=3 {
        match operation().await {
            Ok(value) => return Ok(value),
            Err(error) if is_content_modified_error(&error) && attempt < 3 => {
                tokio::time::sleep(delay).await;
                delay *= 2;
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!("retry loop always returns")
}

fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn initialize_params_configures_rust_analyzer_client_watcher() {
        let params = initialize_params(&test_definition(RUST_ANALYZER_ID));

        assert_eq!(
            params["capabilities"]["window"]["workDoneProgress"],
            serde_json::json!(true)
        );
        assert_eq!(
            params["capabilities"]["workspace"]["configuration"],
            serde_json::json!(true)
        );
        assert_eq!(
            params["capabilities"]["workspace"]["didChangeWatchedFiles"]["dynamicRegistration"],
            serde_json::json!(true)
        );
        assert_eq!(
            params["initializationOptions"],
            serde_json::json!({ "files": { "watcher": "client" } })
        );
    }

    #[test]
    fn client_status_tracks_registered_indexing_progress() {
        let mut status = LspClientStatus::default();
        let token = ProgressToken::String("rustAnalyzer/Roots Scanned".to_string());

        assert!(status.register_progress_token(token.clone()));
        assert!(status.apply_progress(ProgressParams {
            token: token.clone(),
            value: ProgressParamsValue::WorkDone(WorkDoneProgress::Begin(
                lsp_types::WorkDoneProgressBegin {
                    title: "Roots Scanned".to_string(),
                    message: Some("0/408".to_string()),
                    percentage: Some(0),
                    cancellable: Some(false),
                },
            )),
        }));
        assert!(status.apply_progress(ProgressParams {
            token: token.clone(),
            value: ProgressParamsValue::WorkDone(WorkDoneProgress::Report(
                lsp_types::WorkDoneProgressReport {
                    message: Some("166/408".to_string()),
                    percentage: Some(40),
                    cancellable: Some(false),
                },
            )),
        }));

        let runtime = status.runtime_status();

        assert_eq!(runtime.activity_kind, LspActivityKind::Indexing);
        assert_eq!(runtime.activity_title, Some("Roots Scanned".to_string()));
        assert_eq!(runtime.activity_message, Some("166/408".to_string()));
        assert_eq!(runtime.activity_percentage, Some(40));

        assert!(status.apply_progress(ProgressParams {
            token,
            value: ProgressParamsValue::WorkDone(WorkDoneProgress::End(
                lsp_types::WorkDoneProgressEnd::default(),
            )),
        }));
        assert_eq!(status.runtime_status().activity_kind, LspActivityKind::Idle);
    }

    #[test]
    fn client_status_ignores_unregistered_progress() {
        let mut status = LspClientStatus::default();

        let changed = status.apply_progress(ProgressParams {
            token: ProgressToken::String("unknown".to_string()),
            value: ProgressParamsValue::WorkDone(WorkDoneProgress::Begin(
                lsp_types::WorkDoneProgressBegin {
                    title: "Indexing".to_string(),
                    message: None,
                    percentage: None,
                    cancellable: None,
                },
            )),
        });

        assert!(!changed);
        assert_eq!(status.runtime_status().activity_kind, LspActivityKind::Idle);
    }

    #[test]
    fn client_status_records_last_error() {
        let mut status = LspClientStatus::default();

        assert!(status.record_error("LSP server error -32603: url is not a file".to_string()));

        let runtime = status.runtime_status();
        assert_eq!(
            runtime.last_error,
            Some("LSP server error -32603: url is not a file".to_string())
        );
        assert!(runtime.last_error_at.is_some());
    }

    #[test]
    fn workspace_configuration_returns_rust_analyzer_watcher_settings() {
        let params = serde_json::json!({
            "items": [
                { "section": "rust-analyzer" },
                { "section": "rust-analyzer.files" },
                { "section": "rust-analyzer.files.watcher" },
                { "section": "rust-analyzer.cargo" }
            ]
        });

        let result = workspace_configuration_response(Some(&params), RUST_ANALYZER_ID);

        assert_eq!(
            result,
            serde_json::json!([
                { "files": { "watcher": "client" } },
                { "watcher": "client" },
                "client",
                null
            ])
        );
    }

    #[test]
    fn watched_file_event_params_uses_lsp_file_change_type() {
        let path = std::env::current_dir().unwrap().join("src/lib.rs");

        let params = watched_file_event_params(&path, FileChangeType::CHANGED).unwrap();

        assert_eq!(params["changes"][0]["type"], serde_json::json!(2));
        assert!(
            params["changes"][0]["uri"]
                .as_str()
                .unwrap()
                .ends_with("/src/lib.rs")
        );
    }

    #[tokio::test]
    async fn retries_content_modified_errors() {
        let attempts = AtomicUsize::new(0);

        let result = with_content_modified_retries(|| async {
            if attempts.fetch_add(1, Ordering::SeqCst) < 2 {
                return Err(LspRuntimeError::Server {
                    code: -32801,
                    message: "content modified".to_string(),
                });
            }
            Ok(serde_json::json!({"ok": true}))
        })
        .await
        .unwrap();

        assert_eq!(attempts.load(Ordering::SeqCst), 3);
        assert_eq!(result, serde_json::json!({"ok": true}));
    }

    fn test_definition(id: &str) -> LspServerDefinition {
        LspServerDefinition {
            id: id.to_string(),
            display_name: id.to_string(),
            command: id.to_string(),
            args: Vec::new(),
            extensions: vec![".rs".to_string()],
            language_ids: vec!["rust".to_string()],
            workspace_root: std::env::current_dir().unwrap(),
        }
    }
}
