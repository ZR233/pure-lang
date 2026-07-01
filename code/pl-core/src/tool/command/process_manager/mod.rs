use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use pl_protocol::PureError;
use tokio::io::AsyncWriteExt;
use tokio::process::ChildStdin;
use tokio::sync::{Mutex, Notify};
use tokio_util::sync::CancellationToken;

use super::head_tail_buffer::HeadTailBuffer;
use super::shell::shell_command;
use crate::process::{configure_background_command, terminate_process_tree_sync};
use crate::tool::truncation::TruncatedOutput;

mod lifecycle;
mod snapshot;
mod state;
mod stream_io;

use lifecycle::{spawn_lifecycle_task, wait_for_process_activity};
use snapshot::{message_for_state, status_for_state, truncate_text};
use stream_io::{prepare_output_file, read_stderr, read_stdout};

const DEFAULT_MAX_PROCESSES: usize = 16;
const INTERNAL_BUFFER_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone)]
pub(crate) struct CommandProcessManager {
    state: Arc<Mutex<CommandProcessManagerState>>,
}

#[derive(Debug)]
struct CommandProcessManagerState {
    entries: HashMap<String, Arc<CommandProcessEntry>>,
    next_id: u64,
    max_processes: usize,
}

struct CommandProcessEntry {
    process_id: String,
    os_pid: Option<u32>,
    output_file: PathBuf,
    stdin: Mutex<Option<ChildStdin>>,
    state: Mutex<CommandProcessState>,
    notify: Notify,
    output_file_lock: Mutex<()>,
    output_observer: Option<Arc<dyn CommandOutputObserver>>,
}

impl std::fmt::Debug for CommandProcessEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandProcessEntry")
            .field("process_id", &self.process_id)
            .field("os_pid", &self.os_pid)
            .field("output_file", &self.output_file)
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
struct CommandProcessState {
    phase: CommandProcessPhase,
    exit_code: Option<i32>,
    stdout_open: bool,
    stderr_open: bool,
    stdout: HeadTailBuffer,
    stderr: HeadTailBuffer,
    output_revision: u64,
    error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandProcessPhase {
    Running,
    Terminating(CommandTerminationReason),
    Draining(CommandProcessResult),
    Final(CommandProcessResult),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandProcessTransition {
    TimedOut,
    Interrupted,
    ProcessExited { exit_code: Option<i32> },
    ProcessWaitFailed,
    StreamClosed(StreamKind),
}

pub(crate) struct CommandStartRequest {
    pub command: String,
    pub working_directory: PathBuf,
    pub timeout: Duration,
    pub yield_time: Duration,
    pub max_output_chars: usize,
    pub output_file: PathBuf,
    pub cancellation_token: Option<CancellationToken>,
    pub output_observer: Option<Arc<dyn CommandOutputObserver>>,
}

impl std::fmt::Debug for CommandStartRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandStartRequest")
            .field("command", &self.command)
            .field("working_directory", &self.working_directory)
            .field("timeout", &self.timeout)
            .field("yield_time", &self.yield_time)
            .field("max_output_chars", &self.max_output_chars)
            .field("output_file", &self.output_file)
            .field("cancellation_token", &self.cancellation_token.is_some())
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub(crate) struct CommandWriteRequest {
    pub process_id: String,
    pub chars: String,
    pub yield_time: Duration,
    pub max_output_chars: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct CommandOutputSnapshot {
    pub status: String,
    pub process_id: Option<String>,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub stdout: TruncatedOutput,
    pub stderr: TruncatedOutput,
    pub output_file: PathBuf,
    pub message: String,
    pub output_revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandOutputStream {
    Stdout,
    Stderr,
}

/// 命令输出实时观察者。
///
/// `CommandProcessManager` 在读取 stdout/stderr chunk 后调用实现者，
/// 用于把后台进程输出投影到上层 timeline 或其他 live 观察通道。
pub(crate) trait CommandOutputObserver: Send + Sync + 'static {
    fn output_chunk(&self, stream: CommandOutputStream, chunk: &[u8], revision: u64);
}

impl Default for CommandProcessManager {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_PROCESSES)
    }
}

impl Drop for CommandProcessManager {
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
            terminate_process_tree_sync(entry.os_pid);
        }
    }
}

impl CommandProcessManager {
    pub(crate) fn new(max_processes: usize) -> Self {
        Self {
            state: Arc::new(Mutex::new(CommandProcessManagerState {
                entries: HashMap::new(),
                next_id: 0,
                max_processes,
            })),
        }
    }

    pub(crate) async fn start(
        &self,
        request: CommandStartRequest,
    ) -> Result<CommandOutputSnapshot, PureError> {
        prepare_output_file(
            &request.output_file,
            &request.command,
            &request.working_directory,
        )
        .await?;

        let mut command = shell_command(&request.command);
        command.current_dir(&request.working_directory);
        command.stdin(Stdio::piped());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        configure_background_command(&mut command);

        let (process_id, entry, stdout, stderr) = {
            let mut manager_state = self.state.lock().await;
            if manager_state.entries.len() >= manager_state.max_processes {
                return Err(tool_error(
                    "bash",
                    format!(
                        "background process limit reached ({}). Wait for an existing process with write_stdin or let it finish before starting another command.",
                        manager_state.max_processes
                    ),
                ));
            }
            manager_state.next_id = manager_state.next_id.saturating_add(1);
            let next_id = manager_state.next_id;
            let process_id = format!("proc-{next_id}");
            let mut child = command
                .spawn()
                .map_err(|error| tool_error("bash", format!("failed to spawn command: {error}")))?;
            let stdout = child.stdout.take();
            let stderr = child.stderr.take();
            let stdout_open = stdout.is_some();
            let stderr_open = stderr.is_some();
            let stdin = child.stdin.take();
            let entry = Arc::new(CommandProcessEntry {
                process_id: process_id.clone(),
                os_pid: child.id(),
                output_file: request.output_file.clone(),
                stdin: Mutex::new(stdin),
                state: Mutex::new(CommandProcessState::new(stdout_open, stderr_open)),
                notify: Notify::new(),
                output_file_lock: Mutex::new(()),
                output_observer: request.output_observer.clone(),
            });
            manager_state
                .entries
                .insert(process_id.clone(), entry.clone());
            spawn_lifecycle_task(
                entry.clone(),
                child,
                request.timeout,
                request.cancellation_token.clone(),
            );
            (process_id, entry, stdout, stderr)
        };

        if let Some(stdout) = stdout {
            tokio::spawn(read_stdout(entry.clone(), stdout));
        }
        if let Some(stderr) = stderr {
            tokio::spawn(read_stderr(entry.clone(), stderr));
        }
        self.snapshot_after_wait(&process_id, request.yield_time, request.max_output_chars)
            .await
    }

    pub(crate) async fn write_stdin(
        &self,
        request: CommandWriteRequest,
    ) -> Result<CommandOutputSnapshot, PureError> {
        let Some(entry) = self.entry(&request.process_id).await else {
            return Err(tool_error(
                "write_stdin",
                format!(
                    "processId '{}' is not a live process. Re-check the previous bash result and avoid restarting the same command unless the process has ended.",
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
        let snapshot = entry.snapshot(max_output_chars).await;
        if snapshot.process_id.is_none() {
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

    async fn snapshot(&self, max_output_chars: usize) -> CommandOutputSnapshot {
        let state = self.state.lock().await;
        let status = status_for_state(&state);
        let process_id = (status == "running").then(|| self.process_id.clone());
        let stdout = truncate_text(&state.stdout.display_text(), max_output_chars);
        let stderr = truncate_text(&state.stderr.display_text(), max_output_chars);
        let message = message_for_state(&state, process_id.as_deref(), &self.output_file);
        CommandOutputSnapshot {
            status: status.to_string(),
            process_id,
            exit_code: state.exit_code,
            timed_out: state.timed_out(),
            stdout,
            stderr,
            output_file: self.output_file.clone(),
            message,
            output_revision: state.output_revision,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandTerminationReason {
    TimedOut,
    Interrupted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandProcessResult {
    Completed,
    Failed,
    TimedOut,
    Interrupted,
}

impl From<CommandTerminationReason> for CommandProcessResult {
    fn from(reason: CommandTerminationReason) -> Self {
        match reason {
            CommandTerminationReason::TimedOut => Self::TimedOut,
            CommandTerminationReason::Interrupted => Self::Interrupted,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamKind {
    Stdout,
    Stderr,
}

fn tool_error(tool: &str, error: impl std::fmt::Display) -> PureError {
    PureError::ToolExecutionFailed {
        tool: tool.to_string(),
        error: error.to_string(),
    }
}

#[cfg(test)]
mod tests;
