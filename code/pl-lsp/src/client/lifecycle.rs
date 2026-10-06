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

    /// 请求关闭并确认整棵 language-server 进程树已回收。
    ///
    /// 只有在 wait/kill 真实成功后才删除 child owner；未确认回收时保留 owner 并返回
    /// typed error，供 runtime 报告而不是当作 clean。
    pub(crate) async fn shutdown(&self) -> LspResult<()> {
        self.closed.store(true, Ordering::Release);
        self.shutdown_requested.cancel();
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        self.shutdown_connection_locked().await
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
        self.stop_stale_connection_locked().await?;

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
                let _ = self.shutdown_connection_locked().await;
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
            let _ = self.shutdown_connection_locked().await;
            return Err(error);
        }
        if self.closed.load(Ordering::Acquire) {
            let _ = self.shutdown_connection_locked().await;
            return Err(shutting_down_error());
        }
        let initialized_params = serde_json::to_value(InitializedParams {})?;
        if let Err(error) = rpc.notify("initialized", initialized_params).await {
            self.record_last_error(error.to_string()).await;
            let _ = self.shutdown_connection_locked().await;
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

    async fn shutdown_connection_locked(&self) -> LspResult<()> {
        let was_initialized = self.initialized.swap(false, Ordering::Relaxed);
        let mut degraded: Vec<String> = Vec::new();
        if was_initialized && let Ok(rpc) = self.rpc() {
            if let Err(error) = rpc
                .request("shutdown", Value::Null, Duration::from_secs(3))
                .await
            {
                degraded.push(format!("LSP 'shutdown' request failed: {error}"));
            }
            // The outbound channel is capacity-bounded: a stalled writer can in principle
            // block a `notify` forever, which would stop this reclaim path from ever
            // reaching the process-tree owner. Bound it so a cancel is never blocked by an
            // unbounded protocol write.
            match tokio::time::timeout(SHUTDOWN_WAIT_TIMEOUT, rpc.notify("exit", Value::Null)).await
            {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    degraded.push(format!("LSP 'exit' notification failed: {error}"));
                }
                Err(_elapsed) => degraded.push(
                    "LSP 'exit' notification did not complete within the timeout".to_string(),
                ),
            }
        }

        // Reclaim through the existing owned-process mechanism. Only a confirmed wait may
        // remove the child owner; otherwise it is retained for retry/reporting.
        let reclaim = {
            let mut child_guard = self.child.lock().await;
            let confirmed: std::result::Result<Vec<String>, String> = match child_guard.as_mut() {
                None => Ok(Vec::new()),
                Some(child) => {
                    if was_initialized {
                        match tokio::time::timeout(SHUTDOWN_WAIT_TIMEOUT, child.wait()).await {
                            Ok(Ok(())) => Ok(Vec::new()),
                            Ok(Err(error)) => match child.terminate_and_wait().await {
                                Ok(issue) => Ok(prefix_issue(
                                    format!("LSP process wait failed: {error}"),
                                    issue,
                                )),
                                Err(terminate) => {
                                    Err(format!("LSP process wait failed: {error}; {terminate}"))
                                }
                            },
                            Err(_elapsed) => match child.terminate_and_wait().await {
                                Ok(issue) => Ok(prefix_issue(
                                    "LSP process did not exit after shutdown".to_string(),
                                    issue,
                                )),
                                Err(terminate) => Err(format!(
                                    "LSP process did not exit after shutdown; {terminate}"
                                )),
                            },
                        }
                    } else {
                        match child.terminate_and_wait().await {
                            Ok(issue) => Ok(issue.into_iter().collect()),
                            Err(error) => Err(error),
                        }
                    }
                }
            };
            match confirmed {
                Ok(issues) => {
                    degraded.extend(issues);
                    child_guard.take();
                    Ok(())
                }
                Err(error) => Err(error),
            }
        };

        self.connection_generation.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut rpc) = self.rpc.write() {
            rpc.take();
        }
        if let Some(mut transport) = self.transport.lock().await.take() {
            match tokio::task::spawn_blocking(move || transport.close()).await {
                Ok(()) => {}
                Err(join_error) => {
                    degraded.push(format!("LSP transport close task failed: {join_error}"));
                }
            }
        }
        self.opened_files.lock().await.clear();
        self.clear_progress().await;

        // Every recoverable issue must reach the caller instead of being downgraded to a
        // status-only note the runtime can never see.
        for issue in &degraded {
            self.record_last_error(issue.clone()).await;
        }
        match reclaim {
            Err(error) => {
                let mut message =
                    format!("LSP client did not confirm process-tree reclamation: {error}");
                if !degraded.is_empty() {
                    message.push_str("; degraded shutdown: ");
                    message.push_str(&degraded.join("; "));
                }
                Err(LspRuntimeError::Unavailable(message))
            }
            Ok(()) => {
                // Degraded but reclaimed is still not clean: the process tree is gone, so no
                // owner is retained, but the issues above already recorded on the client
                // status are also returned as an error.
                if degraded.is_empty() {
                    Ok(())
                } else {
                    Err(LspRuntimeError::Unavailable(format!(
                        "LSP process tree reclaimed with degraded shutdown: {}",
                        degraded.join("; ")
                    )))
                }
            }
        }
    }

    async fn stop_stale_connection_locked(&self) -> LspResult<()> {
        if self.child.lock().await.is_none()
            && self.transport.lock().await.is_none()
            && self.rpc.read().is_ok_and(|rpc| rpc.is_none())
        {
            return Ok(());
        }
        match self.shutdown_connection_locked().await {
            Ok(()) => Ok(()),
            // A reclaimed-but-degraded connection is safe to replace: the previous process
            // tree is gone, so a fresh server may start. The degraded issues stay on the
            // client status for this internal restart path.
            Err(_) if self.child.lock().await.is_none() => Ok(()),
            Err(error) => Err(error),
        }
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

/// Combine an escalation reason with any issue reported by the confirmed termination.
fn prefix_issue(primary: String, issue: Option<String>) -> Vec<String> {
    let mut issues = vec![primary];
    issues.extend(issue);
    issues
}

fn is_error_stderr_line(line: &str) -> bool {
    let line = line.to_ascii_lowercase();
    line.contains("warn") || line.contains("error")
}
