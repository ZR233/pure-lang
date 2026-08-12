use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use lsp_server::Message;
use lsp_types::{FileChangeType, ProgressParams, PublishDiagnosticsParams};
use serde_json::Value;
use tokio::io::BufReader;
use tokio::process::Command;
use tokio::sync::Mutex;

use crate::client_config::{initialize_params, watched_file_event_params};
use crate::client_retry::is_content_modified_error;
pub(crate) use crate::client_retry::with_content_modified_retries;
use crate::client_server::{
    apply_progress_status, clear_progress_status, record_last_error_status,
    respond_to_server_request,
};
use crate::diagnostics::DiagnosticSink;
use crate::process::{ManagedChild, spawn_background};
use crate::rpc::RpcClient;
use crate::server_definition::LspServerDefinition;
use crate::status::{LspClientRuntimeStatus, LspClientStatus};
use crate::transport::LspTransport;
use crate::types::{LspResult, LspRuntimeError};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const SHUTDOWN_WAIT_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_FILE_SIZE_BYTES: u64 = 10_000_000;

pub(crate) struct LspClient {
    definition: LspServerDefinition,
    child: Mutex<Option<ManagedChild>>,
    transport: Mutex<Option<LspTransport>>,
    rpc: RwLock<Option<RpcClient>>,
    opened_files: Mutex<HashMap<PathBuf, OpenDocument>>,
    initialized: Arc<AtomicBool>,
    connection_generation: Arc<AtomicU64>,
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
            transport: Mutex::new(None),
            rpc: RwLock::new(None),
            opened_files: Mutex::new(HashMap::new()),
            initialized: Arc::new(AtomicBool::new(false)),
            connection_generation: Arc::new(AtomicU64::new(0)),
            start_lock: Mutex::new(()),
            diagnostics,
            status: Arc::new(Mutex::new(LspClientStatus::default())),
        }
    }

    pub async fn request(&self, method: &str, params: Value) -> LspResult<Value> {
        self.ensure_started().await?;
        let result = self.rpc()?.request(method, params, REQUEST_TIMEOUT).await;
        if let Err(error) = &result
            && !is_content_modified_error(error)
        {
            self.record_last_error(error.to_string()).await;
        }
        result
    }

    pub async fn notify(&self, method: &str, params: Value) -> LspResult<()> {
        self.ensure_started().await?;
        self.rpc()?.notify(method, params).await
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
        if self.initialized.swap(false, Ordering::Relaxed)
            && let Ok(rpc) = self.rpc()
        {
            let _ = rpc
                .request("shutdown", Value::Null, Duration::from_secs(3))
                .await;
            let _ = rpc.notify("exit", Value::Null).await;
        }
        if let Some(mut child) = self.child.lock().await.take() {
            match tokio::time::timeout(SHUTDOWN_WAIT_TIMEOUT, child.wait()).await {
                Ok(Ok(_)) => {}
                Ok(Err(_)) | Err(_) => {
                    let kill = child.kill();
                    let _ = std::pin::Pin::from(kill).await;
                }
            }
        }
        self.connection_generation.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut rpc) = self.rpc.write() {
            rpc.take();
        }
        if let Some(mut transport) = self.transport.lock().await.take() {
            let _ = tokio::task::spawn_blocking(move || transport.close()).await;
        }
        self.opened_files.lock().await.clear();
        self.clear_progress().await;
    }

    pub async fn runtime_status(&self) -> LspClientRuntimeStatus {
        self.status.lock().await.runtime_status()
    }

    async fn notify_watched_file_event(&self, path: &Path, typ: FileChangeType) -> LspResult<()> {
        if !self.initialized.load(Ordering::Relaxed) {
            return Ok(());
        }
        self.rpc()?
            .notify(
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
        self.stop_stale_connection().await;

        let mut command = Command::new(&self.definition.command);
        command.args(&self.definition.args);
        command.stdin(Stdio::piped());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        let mut child = spawn_background(command).map_err(|error| {
            let message = format!(
                "Failed to start LSP server '{}': {error}",
                self.definition.id
            );
            LspRuntimeError::Unavailable(message)
        })?;

        let stdin = child.stdin().take().ok_or_else(|| {
            LspRuntimeError::Unavailable("LSP child stdin pipe is unavailable".to_string())
        })?;
        let stdout = child.stdout().take().ok_or_else(|| {
            LspRuntimeError::Unavailable("LSP child stdout pipe is unavailable".to_string())
        })?;
        let stderr = child.stderr().take();
        let (transport, inbound) = LspTransport::spawn(stdin, stdout)?;
        let rpc = RpcClient::new(transport.sender()?);
        let generation = self.connection_generation.fetch_add(1, Ordering::Relaxed) + 1;
        self.spawn_dispatcher(inbound, rpc.clone(), generation);
        self.transport.lock().await.replace(transport);
        self.rpc
            .write()
            .map_err(|_| {
                LspRuntimeError::Unavailable("LSP connection state is poisoned".to_string())
            })?
            .replace(rpc.clone());
        self.child.lock().await.replace(child);

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

        let initialize = rpc
            .request(
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
        if let Err(error) = rpc.notify("initialized", serde_json::json!({})).await {
            self.record_last_error(error.to_string()).await;
            self.shutdown().await;
            return Err(error);
        }
        self.initialized.store(true, Ordering::Relaxed);
        Ok(())
    }

    fn spawn_dispatcher(
        &self,
        mut inbound: tokio::sync::mpsc::Receiver<LspResult<Message>>,
        rpc: RpcClient,
        generation: u64,
    ) {
        let diagnostics = self.diagnostics.clone();
        let status = self.status.clone();
        let updates = self.diagnostics.updates.clone();
        let server_id = self.definition.id.clone();
        let initialized = self.initialized.clone();
        let connection_generation = self.connection_generation.clone();
        tokio::spawn(async move {
            while let Some(message) = inbound.recv().await {
                let message = match message {
                    Ok(message) => message,
                    Err(error) => {
                        rpc.fail_pending(error.to_string()).await;
                        break;
                    }
                };
                match message {
                    Message::Request(request) => {
                        let response =
                            respond_to_server_request(request, &server_id, &status, &updates).await;
                        let _ = rpc.respond(response).await;
                    }
                    Message::Notification(notification) => match notification.method.as_str() {
                        "$/progress" => {
                            if let Ok(params) =
                                serde_json::from_value::<ProgressParams>(notification.params)
                                && apply_progress_status(&status, params).await
                            {
                                let _ = updates.send(());
                            }
                        }
                        "textDocument/publishDiagnostics" => {
                            if let Ok(params) = serde_json::from_value::<PublishDiagnosticsParams>(
                                notification.params,
                            ) {
                                diagnostics.publish(params).await;
                            }
                        }
                        _ => {}
                    },
                    Message::Response(response) => {
                        let result = response.response_result.clone().map_err(|error| {
                            LspRuntimeError::Server {
                                code: i64::from(error.code),
                                message: error.message,
                            }
                        });
                        if let Err(error) = &result
                            && !is_content_modified_error(error)
                            && record_last_error_status(&status, error.to_string()).await
                        {
                            let _ = updates.send(());
                        }
                        rpc.complete(response).await;
                    }
                }
            }
            if clear_progress_status(&status).await {
                let _ = updates.send(());
            }
            rpc.fail_pending(format!("{server_id} connection closed"))
                .await;
            if generation_is_current(&connection_generation, generation) {
                initialized.store(false, Ordering::Relaxed);
            }
        });
    }

    fn rpc(&self) -> LspResult<RpcClient> {
        self.rpc
            .read()
            .map_err(|_| {
                LspRuntimeError::Unavailable("LSP connection state is poisoned".to_string())
            })?
            .clone()
            .ok_or_else(|| LspRuntimeError::Unavailable("LSP transport unavailable".to_string()))
    }

    async fn stop_stale_connection(&self) {
        if self.child.lock().await.is_none()
            && self.transport.lock().await.is_none()
            && self.rpc.read().is_ok_and(|rpc| rpc.is_none())
        {
            return;
        }
        self.shutdown().await;
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

fn generation_is_current(current: &AtomicU64, generation: u64) -> bool {
    current.load(Ordering::Relaxed) == generation
}

fn is_error_stderr_line(line: &str) -> bool {
    let line = line.to_ascii_lowercase();
    line.contains("warn") || line.contains("error")
}
