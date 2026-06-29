use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::time::Duration;

use lsp_types::{FileChangeType, ProgressParams, PublishDiagnosticsParams};
use serde_json::Value;
use tokio::io::BufReader;
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::Mutex;

use crate::client_config::{initialize_params, watched_file_event_params};
use crate::client_retry::is_content_modified_error;
pub(crate) use crate::client_retry::with_content_modified_retries;
use crate::client_server::{
    apply_progress_status, clear_progress_status, record_last_error_status,
    respond_to_server_request,
};
use crate::client_wire::{
    PendingRequests, fail_pending, notify_raw, request_raw, response_id, response_result,
};
use crate::diagnostics::DiagnosticSink;
use crate::framing::read_message;
use crate::process::{configure_background_command, terminate_process_tree};
use crate::server_definition::LspServerDefinition;
use crate::status::{LspClientRuntimeStatus, LspClientStatus};
use crate::types::{LspResult, LspRuntimeError};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const SHUTDOWN_WAIT_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_FILE_SIZE_BYTES: u64 = 10_000_000;

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

    pub async fn open_document(&self, path: &Path, uri: &str) -> LspResult<()> {
        let content = tokio::fs::read_to_string(path).await?;
        let file_size = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
        if file_size > MAX_FILE_SIZE_BYTES {
            return Ok(());
        }
        let text = content;
        let language_id = self.definition.language_for_path(path).unwrap_or("text");
        self.notify(
            "textDocument/didOpen",
            serde_json::json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": language_id,
                    "version": 1,
                    "text": text,
                }
            }),
        )
        .await?;
        self.opened_files.lock().await.insert(
            path.to_path_buf(),
            OpenDocument {
                uri: uri.to_string(),
                version: 1,
            },
        );
        Ok(())
    }

    pub async fn close_document(&self, path: &Path) -> LspResult<()> {
        let document = self.opened_files.lock().await.remove(path);
        if let Some(document) = document {
            self.notify(
                "textDocument/didClose",
                serde_json::json!({
                    "textDocument": {
                        "uri": document.uri,
                    }
                }),
            )
            .await?;
        }
        Ok(())
    }

    pub async fn change_document(&self, path: &Path) -> LspResult<()> {
        let mut opened_files = self.opened_files.lock().await;
        if let Some(document) = opened_files.get_mut(path) {
            document.version += 1;
            let content = tokio::fs::read_to_string(path).await?;
            self.notify(
                "textDocument/didChange",
                serde_json::json!({
                    "textDocument": {
                        "uri": document.uri,
                        "version": document.version,
                    },
                    "contentChanges": [{
                        "text": content,
                    }],
                }),
            )
            .await?;
        }
        Ok(())
    }

    pub async fn file_changed(&self, path: &Path) -> LspResult<()> {
        self.notify_watched_file_event(path, FileChangeType::CHANGED)
            .await
    }

    pub async fn file_deleted(&self, path: &Path) -> LspResult<()> {
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
        command.args(&self.definition.args);
        command.stdin(Stdio::piped());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        configure_background_command(&mut command);
        let mut child = command.spawn().map_err(|error| {
            let message = format!(
                "Failed to start LSP server '{}': {error}",
                self.definition.id
            );
            LspRuntimeError::Unavailable(message)
        })?;

        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take();
        self.stdin.lock().await.replace(stdin);
        self.child.lock().await.replace(child);
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
                            if !line.is_empty()
                                && is_error_stderr_line(line)
                                && record_last_error_status(&status, line.to_string()).await
                            {
                                let _ = updates.send(());
                            }
                        }
                    }
                }
            });
        }

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

fn is_error_stderr_line(line: &str) -> bool {
    let line = line.to_ascii_lowercase();
    line.contains("warn") || line.contains("error")
}
