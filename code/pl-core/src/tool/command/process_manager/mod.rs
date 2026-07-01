use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use pl_protocol::PureError;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{ChildStderr, ChildStdin, ChildStdout};
use tokio::sync::{Mutex, Notify};
use tokio_util::sync::CancellationToken;

use super::head_tail_buffer::HeadTailBuffer;
use super::shell::shell_command;
use crate::process::{
    configure_background_command, terminate_process_tree, terminate_process_tree_sync,
};
use crate::tool::truncation::TruncatedOutput;

mod snapshot;

use snapshot::{message_for_state, status_for_state, truncate_text};

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
            let process_id = format!("proc-{}", manager_state.next_id);
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
                state: Mutex::new(CommandProcessState {
                    phase: CommandProcessPhase::Running,
                    exit_code: None,
                    stdout_open,
                    stderr_open,
                    stdout: HeadTailBuffer::new(INTERNAL_BUFFER_BYTES),
                    stderr: HeadTailBuffer::new(INTERNAL_BUFFER_BYTES),
                    output_revision: 0,
                    error: None,
                }),
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

impl CommandProcessState {
    fn can_accept_input(&self) -> bool {
        matches!(self.phase, CommandProcessPhase::Running)
    }

    fn is_final(&self) -> bool {
        matches!(self.phase, CommandProcessPhase::Final(_))
    }

    fn has_live_child(&self) -> bool {
        matches!(
            self.phase,
            CommandProcessPhase::Running | CommandProcessPhase::Terminating(_)
        )
    }

    fn timed_out(&self) -> bool {
        matches!(
            self.phase,
            CommandProcessPhase::Terminating(CommandTerminationReason::TimedOut)
                | CommandProcessPhase::Draining(CommandProcessResult::TimedOut)
                | CommandProcessPhase::Final(CommandProcessResult::TimedOut)
        )
    }

    fn is_draining_output(&self) -> bool {
        matches!(self.phase, CommandProcessPhase::Draining(_))
    }

    fn termination_reason(&self) -> Option<CommandTerminationReason> {
        match self.phase {
            CommandProcessPhase::Terminating(reason) => Some(reason),
            CommandProcessPhase::Running
            | CommandProcessPhase::Draining(_)
            | CommandProcessPhase::Final(_) => None,
        }
    }

    fn apply_transition(&mut self, transition: CommandProcessTransition) {
        match transition {
            CommandProcessTransition::TimedOut => {
                if matches!(self.phase, CommandProcessPhase::Running) {
                    self.phase =
                        CommandProcessPhase::Terminating(CommandTerminationReason::TimedOut);
                }
            }
            CommandProcessTransition::Interrupted => {
                if matches!(self.phase, CommandProcessPhase::Running) {
                    self.phase =
                        CommandProcessPhase::Terminating(CommandTerminationReason::Interrupted);
                }
            }
            CommandProcessTransition::ProcessExited { exit_code } => {
                self.exit_code = exit_code;
                let result = match self.phase {
                    CommandProcessPhase::Terminating(reason) => reason.into(),
                    CommandProcessPhase::Running
                    | CommandProcessPhase::Draining(_)
                    | CommandProcessPhase::Final(_) => {
                        if self.error.is_some() || exit_code != Some(0) {
                            CommandProcessResult::Failed
                        } else {
                            CommandProcessResult::Completed
                        }
                    }
                };
                self.finish_or_drain(result);
            }
            CommandProcessTransition::ProcessWaitFailed => {
                let result = match self.phase {
                    CommandProcessPhase::Terminating(reason) => reason.into(),
                    CommandProcessPhase::Running
                    | CommandProcessPhase::Draining(_)
                    | CommandProcessPhase::Final(_) => CommandProcessResult::Failed,
                };
                self.finish_or_drain(result);
            }
            CommandProcessTransition::StreamClosed(stream) => {
                match stream {
                    StreamKind::Stdout => self.stdout_open = false,
                    StreamKind::Stderr => self.stderr_open = false,
                }
                if let CommandProcessPhase::Draining(result) = self.phase
                    && self.output_streams_closed()
                {
                    self.phase = CommandProcessPhase::Final(result);
                }
            }
        }
    }

    fn record_error(&mut self, error: String) {
        self.error = Some(error);
        self.phase = match self.phase {
            CommandProcessPhase::Draining(CommandProcessResult::Completed) => {
                CommandProcessPhase::Draining(CommandProcessResult::Failed)
            }
            CommandProcessPhase::Final(CommandProcessResult::Completed) => {
                CommandProcessPhase::Final(CommandProcessResult::Failed)
            }
            phase => phase,
        };
    }

    fn finish_or_drain(&mut self, result: CommandProcessResult) {
        self.phase = if self.output_streams_closed() {
            CommandProcessPhase::Final(result)
        } else {
            CommandProcessPhase::Draining(result)
        };
    }

    fn output_streams_closed(&self) -> bool {
        !self.stdout_open && !self.stderr_open
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

async fn wait_for_process_activity(entry: &CommandProcessEntry, yield_time: Duration) {
    if yield_time.is_zero() {
        return;
    }
    let deadline = Instant::now() + yield_time;
    loop {
        if entry.is_final().await {
            break;
        }
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        let remaining = deadline.saturating_duration_since(now);
        if tokio::time::timeout(remaining, entry.notify.notified())
            .await
            .is_err()
        {
            break;
        }
    }
}

fn spawn_lifecycle_task(
    entry: Arc<CommandProcessEntry>,
    mut child: tokio::process::Child,
    timeout: Duration,
    cancellation_token: Option<CancellationToken>,
) {
    tokio::spawn(async move {
        let outcome = wait_for_lifecycle_outcome(&mut child, timeout, cancellation_token).await;
        let wait_result = match outcome {
            LifecycleOutcome::Exited(result) => result,
            LifecycleOutcome::TimedOut => {
                apply_transition(&entry, CommandProcessTransition::TimedOut).await;
                close_stdin(&entry).await;
                terminate_process_tree(child.id()).await;
                let _ = child.start_kill();
                child.wait().await
            }
            LifecycleOutcome::Interrupted => {
                apply_transition(&entry, CommandProcessTransition::Interrupted).await;
                close_stdin(&entry).await;
                terminate_process_tree(child.id()).await;
                let _ = child.start_kill();
                child.wait().await
            }
        };
        {
            let mut stdin = entry.stdin.lock().await;
            stdin.take();
        }
        match wait_result {
            Ok(status) => {
                apply_transition(
                    &entry,
                    CommandProcessTransition::ProcessExited {
                        exit_code: status.code(),
                    },
                )
                .await;
            }
            Err(error) => {
                {
                    let mut state = entry.state.lock().await;
                    state.record_error(format!("failed to wait for process: {error}"));
                }
                apply_transition(&entry, CommandProcessTransition::ProcessWaitFailed).await;
            }
        }
    });
}

enum LifecycleOutcome {
    Exited(std::io::Result<std::process::ExitStatus>),
    TimedOut,
    Interrupted,
}

async fn wait_for_lifecycle_outcome(
    child: &mut tokio::process::Child,
    timeout: Duration,
    cancellation_token: Option<CancellationToken>,
) -> LifecycleOutcome {
    if let Some(token) = cancellation_token {
        tokio::select! {
            result = child.wait() => LifecycleOutcome::Exited(result),
            _ = tokio::time::sleep(timeout) => LifecycleOutcome::TimedOut,
            _ = token.cancelled() => LifecycleOutcome::Interrupted,
        }
    } else {
        tokio::select! {
            result = child.wait() => LifecycleOutcome::Exited(result),
            _ = tokio::time::sleep(timeout) => LifecycleOutcome::TimedOut,
        }
    }
}

async fn apply_transition(entry: &CommandProcessEntry, transition: CommandProcessTransition) {
    let mut state = entry.state.lock().await;
    state.apply_transition(transition);
    drop(state);
    entry.notify.notify_waiters();
}

async fn close_stdin(entry: &CommandProcessEntry) {
    let mut stdin = entry.stdin.lock().await;
    stdin.take();
}

async fn read_stdout(entry: Arc<CommandProcessEntry>, stdout: ChildStdout) {
    read_stream(entry, stdout, StreamKind::Stdout).await;
}

async fn read_stderr(entry: Arc<CommandProcessEntry>, stderr: ChildStderr) {
    read_stream(entry, stderr, StreamKind::Stderr).await;
}

async fn read_stream<R>(entry: Arc<CommandProcessEntry>, mut reader: R, stream: StreamKind)
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut buffer = [0_u8; 8192];
    loop {
        match reader.read(&mut buffer).await {
            Ok(0) => break,
            Ok(n) => {
                let chunk = &buffer[..n];
                let revision = {
                    let mut state = entry.state.lock().await;
                    state.output_revision = state.output_revision.saturating_add(1);
                    let revision = state.output_revision;
                    match stream {
                        StreamKind::Stdout => state.stdout.push_chunk(chunk),
                        StreamKind::Stderr => state.stderr.push_chunk(chunk),
                    }
                    revision
                };
                if let Some(observer) = &entry.output_observer {
                    observer.output_chunk(stream.into(), chunk, revision);
                }
                if let Err(error) = append_output_chunk(&entry, stream, chunk).await {
                    let mut state = entry.state.lock().await;
                    state.record_error(error);
                }
                entry.notify.notify_waiters();
            }
            Err(error) => {
                let mut state = entry.state.lock().await;
                state.record_error(format!("failed to read process output: {error}"));
                break;
            }
        }
    }
    apply_transition(&entry, CommandProcessTransition::StreamClosed(stream)).await;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamKind {
    Stdout,
    Stderr,
}

impl From<StreamKind> for CommandOutputStream {
    fn from(value: StreamKind) -> Self {
        match value {
            StreamKind::Stdout => Self::Stdout,
            StreamKind::Stderr => Self::Stderr,
        }
    }
}

async fn prepare_output_file(
    output_file: &std::path::Path,
    command: &str,
    working_directory: &std::path::Path,
) -> Result<(), PureError> {
    if let Some(parent) = output_file.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|error| {
            tool_error(
                "bash",
                format!("failed to create output directory: {error}"),
            )
        })?;
    }
    let header = format!(
        "=== COMMAND ===\n{command}\n\n=== CWD ===\n{}\n\n",
        working_directory.display()
    );
    tokio::fs::write(output_file, header.as_bytes())
        .await
        .map_err(|error| tool_error("bash", format!("failed to write output file: {error}")))
}

async fn append_output_chunk(
    entry: &CommandProcessEntry,
    stream: StreamKind,
    chunk: &[u8],
) -> Result<(), String> {
    let _guard = entry.output_file_lock.lock().await;
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&entry.output_file)
        .await
        .map_err(|error| format!("failed to open output file: {error}"))?;
    let label = match stream {
        StreamKind::Stdout => "STDOUT",
        StreamKind::Stderr => "STDERR",
    };
    file.write_all(format!("=== {label} ===\n").as_bytes())
        .await
        .map_err(|error| format!("failed to write output label: {error}"))?;
    file.write_all(chunk)
        .await
        .map_err(|error| format!("failed to write output chunk: {error}"))?;
    if !chunk.ends_with(b"\n") {
        file.write_all(b"\n")
            .await
            .map_err(|error| format!("failed to finish output chunk: {error}"))?;
    }
    Ok(())
}

fn tool_error(tool: &str, error: impl std::fmt::Display) -> PureError {
    PureError::ToolExecutionFailed {
        tool: tool.to_string(),
        error: error.to_string(),
    }
}

#[cfg(test)]
mod tests;
