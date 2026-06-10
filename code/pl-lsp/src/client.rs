use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::time::Duration;

use lsp_types::{DiagnosticSeverity, NumberOrString, PublishDiagnosticsParams};
use serde_json::Value;
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{Mutex, oneshot};

use crate::framing::{encode_message, read_message};
use crate::types::{LspDiagnostic, LspPosition, LspRange, LspResult, LspRuntimeError};
use crate::uri::{file_uri_to_path, normalize_separators};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_FILE_SIZE_BYTES: u64 = 10_000_000;

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
        }
    }

    pub async fn request(&self, method: &str, params: Value) -> LspResult<Value> {
        self.ensure_started().await?;
        request_raw(
            &self.stdin,
            &self.pending,
            &self.next_id,
            method,
            params,
            REQUEST_TIMEOUT,
        )
        .await
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
        if let Some(mut child) = self.child.lock().await.take() {
            let _ = child.kill().await;
        }
        self.opened_files.lock().await.clear();
        self.stdin.lock().await.take();
        self.pending.lock().await.clear();
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
        hide_windows_console(&mut command);
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
            tokio::spawn(async move {
                let mut reader = BufReader::new(stderr);
                loop {
                    let mut line = String::new();
                    match tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line).await {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            let line = line.trim();
                            if !line.is_empty() {
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
                        let _ = respond_to_server_request(&stdin, id, method).await;
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
                    if let Some(sender) = pending.lock().await.remove(&id) {
                        let _ = sender.send(result);
                    }
                }
            }
            fail_pending(
                &pending,
                LspRuntimeError::Unavailable(format!("{server_id} connection closed")),
            )
            .await;
        });
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
) -> LspResult<()> {
    let result = match method {
        "workspace/configuration" => serde_json::json!([]),
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
            "workspace": {
                "configuration": false,
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
    })
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

fn hide_windows_console(command: &mut Command) {
    #[cfg(windows)]
    {
        command.creation_flags(0x08000000);
    }
    #[cfg(not(windows))]
    {
        let _ = command;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use pretty_assertions::assert_eq;

    use super::*;

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
}
