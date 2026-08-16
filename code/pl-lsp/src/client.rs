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
use tokio_util::sync::CancellationToken;

use crate::client_config::{initialize_params, watched_file_event_params};
use crate::client_retry::is_content_modified_error;
pub(crate) use crate::client_retry::with_content_modified_retries;
use crate::client_server::{
    apply_progress_status, clear_progress_status, record_last_error_status,
    respond_to_server_request,
};
use crate::diagnostics::DiagnosticSink;
use crate::driver::LspServerDriver;
use crate::process::{ManagedChild, spawn_background};
use crate::resolved::ResolvedLspServer;
use crate::rpc::RpcClient;
use crate::status::{LspClientRuntimeStatus, LspClientStatus};
use crate::transport::LspTransport;
use crate::types::{LspResult, LspRuntimeError};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const SHUTDOWN_WAIT_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_FILE_SIZE_BYTES: u64 = 10_000_000;

pub(crate) struct LspClient {
    server: ResolvedLspServer,
    driver: Arc<dyn LspServerDriver>,
    child: Mutex<Option<ManagedChild>>,
    transport: Mutex<Option<LspTransport>>,
    rpc: RwLock<Option<RpcClient>>,
    opened_files: Mutex<HashMap<PathBuf, OpenDocument>>,
    document_sync: Mutex<()>,
    initialized: Arc<AtomicBool>,
    connection_generation: Arc<AtomicU64>,
    lifecycle_lock: Mutex<()>,
    shutdown_requested: CancellationToken,
    closed: AtomicBool,
    diagnostics: DiagnosticSink,
    status: Arc<Mutex<LspClientStatus>>,
}

impl std::fmt::Debug for LspClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LspClient")
            .field("server_id", &self.server.id)
            .field("workspace_root", &self.server.workspace_root)
            .field("initialized", &self.initialized.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
struct OpenDocument {
    uri: String,
    version: i32,
    content: String,
}

impl LspClient {
    pub(crate) async fn start(&self) -> LspResult<()> {
        self.ensure_started().await
    }
    pub fn new(
        server: ResolvedLspServer,
        driver: Arc<dyn LspServerDriver>,
        diagnostics: DiagnosticSink,
    ) -> Self {
        Self {
            server,
            driver,
            child: Mutex::new(None),
            transport: Mutex::new(None),
            rpc: RwLock::new(None),
            opened_files: Mutex::new(HashMap::new()),
            document_sync: Mutex::new(()),
            initialized: Arc::new(AtomicBool::new(false)),
            connection_generation: Arc::new(AtomicU64::new(0)),
            lifecycle_lock: Mutex::new(()),
            shutdown_requested: CancellationToken::new(),
            closed: AtomicBool::new(false),
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
        let _sync = self.document_sync.lock().await;
        let document = self.opened_files.lock().await.get(path).cloned();
        if let Some(document) = document {
            if document.content == content {
                return Ok(());
            }
            let next_version = document.version + 1;
            self.notify(
                "textDocument/didChange",
                serde_json::json!({
                    "textDocument": {
                        "uri": document.uri,
                        "version": next_version,
                    },
                    "contentChanges": [{
                        "text": content.clone(),
                    }],
                }),
            )
            .await?;
            if let Some(document) = self.opened_files.lock().await.get_mut(path) {
                document.version = next_version;
                document.content = content;
            }
        } else {
            let language_id = self.server.language_for_path(path).unwrap_or("text");
            self.notify(
                "textDocument/didOpen",
                serde_json::json!({
                    "textDocument": {
                        "uri": uri,
                        "languageId": language_id,
                        "version": 1,
                        "text": content.clone(),
                    }
                }),
            )
            .await?;
            self.opened_files.lock().await.insert(
                path.to_path_buf(),
                OpenDocument {
                    uri: uri.to_string(),
                    version: 1,
                    content,
                },
            );
        }
        Ok(())
    }

    pub async fn close_document(&self, path: &Path) -> LspResult<()> {
        let _sync = self.document_sync.lock().await;
        let document = self.opened_files.lock().await.get(path).cloned();
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
            self.opened_files.lock().await.remove(path);
        }
        Ok(())
    }

    pub async fn change_document(&self, path: &Path) -> LspResult<()> {
        let content = tokio::fs::read_to_string(path).await?;
        let _sync = self.document_sync.lock().await;
        let document = self.opened_files.lock().await.get(path).cloned();
        if let Some(document) = document {
            if document.content == content {
                return Ok(());
            }
            let next_version = document.version + 1;
            self.notify(
                "textDocument/didChange",
                serde_json::json!({
                    "textDocument": {
                        "uri": document.uri,
                        "version": next_version,
                    },
                    "contentChanges": [{
                        "text": content.clone(),
                    }],
                }),
            )
            .await?;
            if let Some(document) = self.opened_files.lock().await.get_mut(path) {
                document.version = next_version;
                document.content = content;
            }
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
        self.closed.store(true, Ordering::Release);
        self.shutdown_requested.cancel();
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        self.shutdown_connection_locked().await;
    }

    async fn shutdown_connection_locked(&self) {
        let was_initialized = self.initialized.swap(false, Ordering::Relaxed);
        if was_initialized && let Ok(rpc) = self.rpc() {
            let _ = rpc
                .request("shutdown", Value::Null, Duration::from_secs(3))
                .await;
            let _ = rpc.notify("exit", Value::Null).await;
        }
        if let Some(mut child) = self.child.lock().await.take() {
            if was_initialized {
                match tokio::time::timeout(SHUTDOWN_WAIT_TIMEOUT, child.wait()).await {
                    Ok(Ok(_)) => {}
                    Ok(Err(_)) | Err(_) => {
                        let kill = child.kill();
                        let _ = std::pin::Pin::from(kill).await;
                    }
                }
            } else if !child.try_wait().is_ok_and(|status| status.is_some()) {
                let kill = child.kill();
                let _ = std::pin::Pin::from(kill).await;
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

    #[cfg(test)]
    pub async fn child_id_for_test(&self) -> Option<u32> {
        self.child
            .lock()
            .await
            .as_ref()
            .and_then(|child| child.id())
    }

    pub async fn wait_until_idle(&self, timeout: Duration) {
        let mut updates = self.diagnostics.updates.subscribe();
        let wait = async {
            loop {
                if self.status.lock().await.is_idle_after_observed_activity() {
                    return;
                }
                match updates.recv().await {
                    Ok(()) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                }
            }
        };
        let _ = tokio::time::timeout(timeout, wait).await;
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
        if self.closed.load(Ordering::Acquire) {
            return Err(LspRuntimeError::Unavailable(
                "LSP client is shutting down".to_string(),
            ));
        }
        if self.initialized.load(Ordering::Relaxed) {
            return Ok(());
        }
        let _guard = self.lifecycle_lock.lock().await;
        if self.closed.load(Ordering::Acquire) {
            return Err(LspRuntimeError::Unavailable(
                "LSP client is shutting down".to_string(),
            ));
        }
        if self.initialized.load(Ordering::Relaxed) {
            return Ok(());
        }
        self.stop_stale_connection_locked().await;

        let mut command = Command::new(&self.server.program);
        command.args(&self.server.args);
        command.stdin(Stdio::piped());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        let mut child = spawn_background(command).map_err(|error| {
            let message = format!("Failed to start LSP server '{}': {error}", self.server.id);
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

        let initialize = tokio::select! {
            biased;
            _ = self.shutdown_requested.cancelled() => {
                self.shutdown_connection_locked().await;
                return Err(LspRuntimeError::Unavailable(
                    "LSP client is shutting down".to_string(),
                ));
            }
            initialize = rpc.request(
                "initialize",
                initialize_params(&self.server, self.driver.initialization_options()),
                STARTUP_TIMEOUT,
            ) => initialize,
        };
        if let Err(error) = initialize {
            self.record_last_error(error.to_string()).await;
            self.shutdown_connection_locked().await;
            return Err(error);
        }
        if self.closed.load(Ordering::Acquire) {
            self.shutdown_connection_locked().await;
            return Err(LspRuntimeError::Unavailable(
                "LSP client is shutting down".to_string(),
            ));
        }
        if let Err(error) = rpc.notify("initialized", serde_json::json!({})).await {
            self.record_last_error(error.to_string()).await;
            self.shutdown_connection_locked().await;
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
        let driver = self.driver.clone();
        let server_id = self.server.id.clone();
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
                            respond_to_server_request(request, driver.as_ref(), &status, &updates)
                                .await;
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

    async fn stop_stale_connection_locked(&self) {
        if self.child.lock().await.is_none()
            && self.transport.lock().await.is_none()
            && self.rpc.read().is_ok_and(|rpc| rpc.is_none())
        {
            return;
        }
        self.shutdown_connection_locked().await;
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::process::Stdio;

    use super::*;

    #[tokio::test]
    async fn shutdown_waits_for_in_flight_start_before_taking_child() {
        let client = Arc::new(test_client());
        let lifecycle_guard = client.lifecycle_lock.lock().await;
        let shutting_down = client.clone();
        let shutdown = tokio::spawn(async move { shutting_down.shutdown().await });
        while !client.closed.load(Ordering::Acquire) {
            tokio::task::yield_now().await;
        }

        let mut command = Command::new(std::env::current_exe().expect("current test executable"));
        command.arg("--list");
        command.stdin(Stdio::null());
        command.stdout(Stdio::null());
        command.stderr(Stdio::null());
        let child = command.spawn().expect("spawn short-lived test child");
        client.child.lock().await.replace(Box::new(child));
        drop(lifecycle_guard);

        tokio::time::timeout(Duration::from_secs(5), shutdown)
            .await
            .expect("shutdown must finish")
            .expect("shutdown task must not panic");
        assert!(client.child.lock().await.is_none());
        assert!(
            matches!(
                client.ensure_started().await,
                Err(LspRuntimeError::Unavailable(message))
                    if message == "LSP client is shutting down"
            ),
            "a terminally closed client must not start another child"
        );
    }

    fn test_client() -> LspClient {
        let (updates, _) = tokio::sync::broadcast::channel(4);
        let workspace_root = std::env::current_dir().expect("current test directory");
        let server = ResolvedLspServer {
            id: "test-lsp".to_string(),
            display_name: "Test LSP".to_string(),
            program: "unused-test-command".to_string(),
            args: Vec::new(),
            extensions: vec![".pure".to_string()],
            language_ids: vec!["purelang".to_string()],
            operations: Vec::new(),
            workspace_root: workspace_root.clone(),
        };
        let diagnostics = DiagnosticSink::new(
            server.id.clone(),
            workspace_root,
            Arc::new(Mutex::new(HashMap::new())),
            updates,
        );
        LspClient::new(server, test_driver(), diagnostics)
    }

    fn test_driver() -> Arc<dyn LspServerDriver> {
        struct StubDriver;

        impl LspServerDriver for StubDriver {
            fn probe<'a>(
                &'a self,
                _command: &'a crate::driver::LspResolvedCommand,
            ) -> futures::future::BoxFuture<'a, crate::driver::LspProbeOutcome> {
                futures::FutureExt::boxed(std::future::ready(
                    crate::driver::LspProbeOutcome::Failed {
                        message: "stub".to_string(),
                    },
                ))
            }

            fn repair<'a>(
                &'a self,
                _component: &'a crate::types::LspMissingComponent,
            ) -> futures::future::BoxFuture<'a, Result<(), crate::driver::LspRepairError>>
            {
                futures::FutureExt::boxed(std::future::ready(Err(
                    crate::driver::LspRepairError::NotSupported,
                )))
            }
        }

        Arc::new(StubDriver)
    }
}
