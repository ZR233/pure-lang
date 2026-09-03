use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use pl_protocol::PureError;
use tokio::io::AsyncWriteExt;
use tokio::sync::{Mutex, Notify};
use tokio_util::sync::CancellationToken;

use super::backend::{
    CommandBackend, CommandOutputSizes, CommandOutputTarget, CommandSpawnRequest, CommandWriter,
};
use super::head_tail_buffer::HeadTailBuffer;
use crate::tool::tool_error;
use crate::tool::truncation::TruncatedOutput;

mod lifecycle;
mod snapshot;
mod state;
mod stream_io;

use lifecycle::{spawn_lifecycle_task, wait_for_process_activity};
use snapshot::{message_for_state, truncate_text};
use state::CommandProcessTransition;
pub use state::{
    CommandProcessFailure, CommandProcessFinalResult, CommandProcessLifecycle,
    CommandTerminationReason, DrainingCommandProcess, FinalCommandProcess, RunningCommandProcess,
    TerminatingCommandProcess,
};
use stream_io::{read_stderr, read_stdout};

const DEFAULT_MAX_PROCESSES: usize = 16;
const INTERNAL_BUFFER_BYTES: usize = 64 * 1024;
static NEXT_PROCESS_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug)]
pub struct CommandProcessManager<B>
where
    B: CommandBackend,
{
    state: Arc<Mutex<CommandProcessManagerState>>,
    backend: Arc<B>,
}

impl<B> Clone for CommandProcessManager<B>
where
    B: CommandBackend,
{
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            backend: self.backend.clone(),
        }
    }
}

#[derive(Debug)]
struct CommandProcessManagerState {
    entries: HashMap<String, Arc<CommandProcessEntry>>,
    starting: usize,
    max_processes: usize,
}

struct CommandProcessEntry {
    process_id: String,
    os_pid: Option<u32>,
    output_target: CommandOutputTarget,
    stdin: Mutex<Option<CommandWriter>>,
    state: Mutex<CommandProcessState>,
    notify: Notify,
    output_observer: Option<Arc<dyn CommandOutputObserver>>,
}

impl std::fmt::Debug for CommandProcessEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandProcessEntry")
            .field("process_id", &self.process_id)
            .field("os_pid", &self.os_pid)
            .field("output_file", &self.output_target.model_file())
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
struct CommandProcessState {
    lifecycle: CommandProcessLifecycle,
    stdout_open: bool,
    stderr_open: bool,
    stdout: HeadTailBuffer,
    stderr: HeadTailBuffer,
    pending_stdout: HeadTailBuffer,
    pending_stderr: HeadTailBuffer,
    output_revision: u64,
}

pub struct CommandStartRequest {
    pub command: String,
    pub cwd: Option<PathBuf>,
    pub allow_workspace_escape: bool,
    pub timeout: Duration,
    pub yield_time: Duration,
    pub max_output_chars: usize,
    pub session_id: String,
    pub tool_id: String,
    pub call_id: String,
    pub cancellation_token: Option<CancellationToken>,
    pub output_observer: Option<Arc<dyn CommandOutputObserver>>,
}

impl std::fmt::Debug for CommandStartRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandStartRequest")
            .field("command", &self.command)
            .field("cwd", &self.cwd)
            .field("allow_workspace_escape", &self.allow_workspace_escape)
            .field("timeout", &self.timeout)
            .field("yield_time", &self.yield_time)
            .field("max_output_chars", &self.max_output_chars)
            .field("session_id", &self.session_id)
            .field("tool_id", &self.tool_id)
            .field("call_id", &self.call_id)
            .field("cancellation_token", &self.cancellation_token.is_some())
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub struct CommandWriteRequest {
    pub process_id: String,
    pub chars: String,
    pub yield_time: Duration,
    pub max_output_chars: usize,
}

#[derive(Debug, Clone)]
pub struct CommandOutputSnapshot {
    pub state: CommandProcessLifecycle,
    pub process_id: Option<String>,
    pub stdout: TruncatedOutput,
    pub stderr: TruncatedOutput,
    pub output_file: PathBuf,
    pub capture_file: PathBuf,
    pub message: String,
    pub output_revision: u64,
    pub output_artifacts: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandOutputStream {
    Stdout,
    Stderr,
}

/// 命令输出实时观察者。
///
/// `CommandProcessManager` 在读取 stdout/stderr chunk 后调用实现者，
/// 用于把后台进程输出投影到上层 timeline 或其他 live 观察通道。
pub trait CommandOutputObserver: Send + Sync + 'static {
    fn output_chunk(&self, stream: CommandOutputStream, chunk: &[u8], revision: u64);
}

impl<B> Drop for CommandProcessManager<B>
where
    B: CommandBackend,
{
    fn drop(&mut self) {
        if Arc::strong_count(&self.state) != 1 {
            return;
        }
        let Ok(state) = self.state.try_lock() else {
            return;
        };
        for entry in state.entries.values() {
            if let Ok(process_state) = entry.state.try_lock()
                && !process_state.has_live_child()
            {
                continue;
            }
            self.backend.terminate_sync(&entry.process_id, entry.os_pid);
        }
    }
}

impl<B> CommandProcessManager<B>
where
    B: CommandBackend,
{
    pub fn new(backend: Arc<B>) -> Self {
        Self::with_max_processes(backend, DEFAULT_MAX_PROCESSES)
    }

    pub fn with_max_processes(backend: Arc<B>, max_processes: usize) -> Self {
        Self {
            state: Arc::new(Mutex::new(CommandProcessManagerState {
                entries: HashMap::new(),
                starting: 0,
                max_processes,
            })),
            backend,
        }
    }

    pub async fn start(
        &self,
        request: CommandStartRequest,
    ) -> Result<CommandOutputSnapshot, PureError> {
        let working_directory = self
            .backend
            .resolve_cwd(request.cwd.as_deref(), request.allow_workspace_escape)
            .await
            .map_err(|error| tool_error("exec", error))?;
        let output_target = self
            .backend
            .output_target(
                &request.session_id,
                &request.tool_id,
                &request.call_id,
                &request.command,
            )
            .await
            .map_err(|error| tool_error("exec", error))?;
        self.backend
            .prepare_output(&output_target, &request.command, &working_directory)
            .await
            .map_err(|error| tool_error("exec", error))?;

        let process_id = self.reserve_process_id().await?;
        let child = self
            .backend
            .spawn(CommandSpawnRequest {
                process_id: process_id.clone(),
                command: request.command,
                cwd: working_directory,
                output_target: output_target.clone(),
            })
            .await;
        let mut child = match child {
            Ok(child) => child,
            Err(error) => {
                self.release_start_reservation().await;
                return Err(tool_error("exec", error));
            }
        };
        let stdout = child.take_stdout();
        let stderr = child.take_stderr();
        let stdout_open = stdout.is_some();
        let stderr_open = stderr.is_some();
        let stdin = child.take_stdin();
        let entry = Arc::new(CommandProcessEntry {
            process_id: process_id.clone(),
            os_pid: child.host_pid(),
            output_target,
            stdin: Mutex::new(stdin),
            state: Mutex::new(CommandProcessState::new(stdout_open, stderr_open)),
            notify: Notify::new(),
            output_observer: request.output_observer,
        });
        {
            let mut state = self.state.lock().await;
            state.starting = state.starting.saturating_sub(1);
            state.entries.insert(process_id.clone(), entry.clone());
        }
        spawn_lifecycle_task(
            entry.clone(),
            child,
            request.timeout,
            request.cancellation_token,
            self.backend.clone(),
        );

        if let Some(stdout) = stdout {
            tokio::spawn(read_stdout(entry.clone(), stdout, self.backend.clone()));
        }
        if let Some(stderr) = stderr {
            tokio::spawn(read_stderr(entry.clone(), stderr, self.backend.clone()));
        }
        self.snapshot_after_wait(&process_id, request.yield_time, request.max_output_chars)
            .await
    }

    pub async fn write_stdin(
        &self,
        request: CommandWriteRequest,
    ) -> Result<CommandOutputSnapshot, PureError> {
        let Some(entry) = self.entry(&request.process_id).await else {
            return Err(tool_error(
                "write_stdin",
                format!(
                    "processId '{}' is not a live process. Re-check the previous exec result and avoid restarting the same command unless the process has ended.",
                    request.process_id
                ),
            ));
        };
        if !request.chars.is_empty() {
            if !entry.can_accept_input().await {
                return self
                    .snapshot_after_wait(
                        &request.process_id,
                        Duration::ZERO,
                        request.max_output_chars,
                    )
                    .await;
            }
            let mut stdin = entry.stdin.lock().await;
            let Some(stdin) = stdin.as_mut() else {
                return Err(tool_error(
                    "write_stdin",
                    format!(
                        "processId '{}' does not accept stdin. Poll it with empty chars instead.",
                        request.process_id
                    ),
                ));
            };
            stdin
                .write_all(request.chars.as_bytes())
                .await
                .map_err(|error| {
                    tool_error("write_stdin", format!("failed to write stdin: {error}"))
                })?;
            stdin.flush().await.map_err(|error| {
                tool_error("write_stdin", format!("failed to flush stdin: {error}"))
            })?;
        }

        self.snapshot_after_wait(
            &request.process_id,
            request.yield_time,
            request.max_output_chars,
        )
        .await
    }

    async fn reserve_process_id(&self) -> Result<String, PureError> {
        let mut state = self.state.lock().await;
        if state.entries.len().saturating_add(state.starting) >= state.max_processes {
            return Err(tool_error(
                "exec",
                format!(
                    "background process limit reached ({}). Wait for an existing process with write_stdin or let it finish before starting another command.",
                    state.max_processes
                ),
            ));
        }
        let next_id = NEXT_PROCESS_ID
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
                next.checked_add(1)
            })
            .map_err(|_| {
                tool_error(
                    "exec",
                    "process id space exhausted; restart Pure before starting another command"
                        .to_string(),
                )
            })?;
        state.starting = state.starting.saturating_add(1);
        Ok(format!("proc-{next_id}"))
    }

    async fn release_start_reservation(&self) {
        let mut state = self.state.lock().await;
        state.starting = state.starting.saturating_sub(1);
    }

    async fn entry(&self, process_id: &str) -> Option<Arc<CommandProcessEntry>> {
        self.state.lock().await.entries.get(process_id).cloned()
    }

    async fn snapshot_after_wait(
        &self,
        process_id: &str,
        yield_time: Duration,
        max_output_chars: usize,
    ) -> Result<CommandOutputSnapshot, PureError> {
        let Some(entry) = self.entry(process_id).await else {
            return Err(tool_error(
                "write_stdin",
                format!("processId '{process_id}' is not a live process"),
            ));
        };
        wait_for_process_activity(&entry, yield_time).await;
        self.backend
            .publish_output(&entry.output_target)
            .await
            .map_err(|error| tool_error("exec", error))?;
        let (mut snapshot, sizes) = entry.snapshot(max_output_chars).await;
        if snapshot.process_id.is_none() {
            snapshot.output_artifacts = self
                .backend
                .collect_output_artifacts(&entry.output_target, sizes)
                .await
                .map_err(|error| tool_error("exec", error))?;
            self.state.lock().await.entries.remove(process_id);
        }
        Ok(snapshot)
    }
}

impl CommandProcessEntry {
    async fn can_accept_input(&self) -> bool {
        let state = self.state.lock().await;
        state.can_accept_input()
    }

    async fn is_final(&self) -> bool {
        self.state.lock().await.is_final()
    }

    async fn snapshot(
        &self,
        max_output_chars: usize,
    ) -> (CommandOutputSnapshot, CommandOutputSizes) {
        let mut state = self.state.lock().await;
        let process_id = (!state.lifecycle.is_final()).then(|| self.process_id.clone());
        let message = message_for_state(
            &state,
            process_id.as_deref(),
            self.output_target.model_file(),
        );
        let sizes = CommandOutputSizes {
            stdout_bytes: state.stdout.total_bytes() as u64,
            stderr_bytes: state.stderr.total_bytes() as u64,
        };
        let stdout = truncate_text(&state.pending_stdout.take_display_text(), max_output_chars);
        let stderr = truncate_text(&state.pending_stderr.take_display_text(), max_output_chars);
        (
            CommandOutputSnapshot {
                state: state.lifecycle.clone(),
                process_id,
                stdout,
                stderr,
                output_file: self.output_target.model_file().to_path_buf(),
                capture_file: self.output_target.capture_file().to_path_buf(),
                message,
                output_revision: state.output_revision,
                output_artifacts: Vec::new(),
            },
            sizes,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamKind {
    Stdout,
    Stderr,
}
