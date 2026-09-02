use std::process::Stdio;
use std::sync::atomic::Ordering;
use std::time::Duration;

use lsp_types::InitializedParams;
use serde_json::Value;
use tokio::io::BufReader;
use tokio::process::Command;

use super::configuration::initialize_params;
use super::connection::LspClient;
use super::message::{clear_progress_status, record_last_error_status};
use super::rpc::RpcClient;
use super::status::LspClientRuntimeStatus;
use super::transport::LspTransport;
use crate::host::{LspChild, LspHostSpawnRequest, spawn_background};
use crate::runtime::{LspResult, LspRuntimeError};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const SHUTDOWN_WAIT_TIMEOUT: Duration = Duration::from_secs(2);

impl LspClient {
    pub(crate) async fn start(&self) -> LspResult<()> {
        self.ensure_started().await
    }

    pub(crate) async fn shutdown(&self) {
        self.closed.store(true, Ordering::Release);
        self.shutdown_requested.cancel();
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        self.shutdown_connection_locked().await;
    }

    pub(crate) async fn runtime_status(&self) -> LspClientRuntimeStatus {
        self.status.lock().await.runtime_status()
    }

    pub(crate) async fn wait_until_idle(&self, timeout: Duration) {
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

    pub(super) async fn ensure_started(&self) -> LspResult<()> {
        if self.closed.load(Ordering::Acquire) {
            return Err(shutting_down_error());
        }
        if self.initialized.load(Ordering::Relaxed) {
            return Ok(());
        }
        let _guard = self.lifecycle_lock.lock().await;
        if self.closed.load(Ordering::Acquire) {
            return Err(shutting_down_error());
        }
        if self.initialized.load(Ordering::Relaxed) {
            return Ok(());
        }
        self.stop_stale_connection_locked().await;

        let mut child = self.spawn_server().await?;
        let stdin = child.take_stdin().ok_or_else(|| {
            LspRuntimeError::Unavailable("LSP child stdin pipe is unavailable".to_string())
        })?;
        let stdout = child.take_stdout().ok_or_else(|| {
            LspRuntimeError::Unavailable("LSP child stdout pipe is unavailable".to_string())
        })?;
        let stderr = child.take_stderr();
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
        self.observe_stderr(stderr);

        let initialize = tokio::select! {
            biased;
            _ = self.shutdown_requested.cancelled() => {
                self.shutdown_connection_locked().await;
                return Err(shutting_down_error());
            }
            initialize = rpc.request(
                "initialize",
                initialize_params(&self.server, self.driver.initialization_options())?,
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
            return Err(shutting_down_error());
        }
        let initialized_params = serde_json::to_value(InitializedParams {})?;
        if let Err(error) = rpc.notify("initialized", initialized_params).await {
            self.record_last_error(error.to_string()).await;
            self.shutdown_connection_locked().await;
            return Err(error);
        }
        self.initialized.store(true, Ordering::Relaxed);
        Ok(())
    }

    async fn spawn_server(&self) -> LspResult<LspChild> {
        if let Some(host) = &self.host {
            return host
                .spawn(LspHostSpawnRequest {
                    process_id: format!("lsp-{}", self.server.id),
                    program: self.server.program.clone(),
                    args: self.server.args.clone(),
                    cwd: self.server.workspace_root.clone(),
                })
                .await
                .map(LspChild::Hosted)
                .map_err(|error| {
                    LspRuntimeError::Unavailable(format!(
                        "Failed to start LSP server '{}': {error}",
                        self.server.id
                    ))
                });
        }
        let mut command = Command::new(&self.server.program);
        command.args(&self.server.args);
        command.stdin(Stdio::piped());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        spawn_background(command)
            .map(LspChild::Local)
            .map_err(|error| {
                LspRuntimeError::Unavailable(format!(
                    "Failed to start LSP server '{}': {error}",
                    self.server.id
                ))
            })
    }

    fn observe_stderr(&self, stderr: Option<crate::host::LspHostReader>) {
        let Some(stderr) = stderr else {
            return;
        };
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
                        let _ = child.kill().await;
                    }
                }
            } else if !child.has_exited() {
                let _ = child.kill().await;
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

    async fn stop_stale_connection_locked(&self) {
        if self.child.lock().await.is_none()
            && self.transport.lock().await.is_none()
            && self.rpc.read().is_ok_and(|rpc| rpc.is_none())
        {
            return;
        }
        self.shutdown_connection_locked().await;
    }

    pub(super) async fn record_last_error(&self, message: String) {
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

fn shutting_down_error() -> LspRuntimeError {
    LspRuntimeError::Unavailable("LSP client is shutting down".to_string())
}

fn is_error_stderr_line(line: &str) -> bool {
    let line = line.to_ascii_lowercase();
    line.contains("warn") || line.contains("error")
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use tokio::sync::Mutex;

    use super::*;
    use crate::client::DiagnosticSink;
    use crate::driver::{LspProbeOutcome, LspRepairError, LspResolvedCommand, LspServerDriver};
    use crate::host::LspHostBackend;
    use crate::runtime::{LspMissingComponent, ResolvedLspServer};

    #[tokio::test]
    async fn shutdown_waits_for_in_flight_start_before_releasing_the_child() {
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
        client
            .child
            .lock()
            .await
            .replace(LspChild::Local(Box::new(child)));
        drop(lifecycle_guard);

        tokio::time::timeout(Duration::from_secs(5), shutdown)
            .await
            .expect("shutdown must finish")
            .expect("shutdown task must not panic");
        assert!(client.child.lock().await.is_none());
        assert!(matches!(
            client.ensure_started().await,
            Err(LspRuntimeError::Unavailable(message))
                if message == "LSP client is shutting down"
        ));
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
        LspClient::new(server, Arc::new(StubDriver), diagnostics, None)
    }

    #[derive(Debug)]
    struct StubDriver;

    impl LspServerDriver for StubDriver {
        fn probe<'a>(
            &'a self,
            _command: &'a LspResolvedCommand,
            _host: Option<&'a dyn LspHostBackend>,
        ) -> futures::future::BoxFuture<'a, LspProbeOutcome> {
            futures::FutureExt::boxed(std::future::ready(LspProbeOutcome::Failed {
                message: "stub".to_string(),
            }))
        }

        fn repair<'a>(
            &'a self,
            _component: &'a LspMissingComponent,
            _host: Option<&'a dyn LspHostBackend>,
        ) -> futures::future::BoxFuture<'a, Result<(), LspRepairError>> {
            futures::FutureExt::boxed(std::future::ready(Err(LspRepairError::NotSupported)))
        }
    }
}
