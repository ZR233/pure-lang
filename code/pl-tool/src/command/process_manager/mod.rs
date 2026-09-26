use std::collections::{HashMap, HashSet, VecDeque};
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
use crate::tool_error;
use pl_output::TruncatedOutput;

mod lifecycle;
mod snapshot;
mod state;
mod stream_io;

use lifecycle::{CommandLifecycle, spawn_lifecycle_task, wait_for_process_activity};
use snapshot::{message_for_state, truncate_text};
use state::CommandProcessTransition;
pub use state::{
    CaptureRepair, CaptureRepairChunk, CommandCaptureFailure, CommandProcessFailure,
    CommandProcessFinalResult, CommandProcessLifecycle, CommandTerminationReason,
    DrainingCommandProcess, FinalCommandProcess, RunningCommandProcess, TerminatingCommandProcess,
};
use stream_io::{read_stderr, read_stdout, run_capture_writer};

const DEFAULT_MAX_PROCESSES: usize = 16;
const INTERNAL_BUFFER_BYTES: usize = 64 * 1024;
/// Largest total capture one command operation may write to disk.
///
/// stdout and stderr share this one budget, so a command that never stops writing cannot grow the
/// capture file without bound. It matches the reliable output ceiling core reserves for one
/// operation, so the durable capture and the output core can retain stay the same order of magnitude
/// instead of the capture silently exceeding what the rest of the pipeline can hold.
pub const MAX_CAPTURE_BYTES: u64 = 16 * 1024 * 1024;
static NEXT_PROCESS_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug)]
pub struct CommandProcessManager<B>
where
    B: CommandBackend,
{
    state: Arc<Mutex<CommandProcessManagerState>>,
    backend: Arc<B>,
    lifetime: Arc<ManagerLifetime>,
}

impl<B> Clone for CommandProcessManager<B>
where
    B: CommandBackend,
{
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            backend: self.backend.clone(),
            lifetime: self.lifetime.clone(),
        }
    }
}

#[derive(Debug)]
struct CommandProcessManagerState {
    entries: HashMap<String, Arc<CommandProcessEntry>>,
    starting: usize,
    task_reservations: HashSet<String>,
    max_processes: usize,
}

struct CommandProcessEntry {
    process_id: String,
    output_target: CommandOutputTarget,
    stdin: Mutex<Option<CommandWriter>>,
    state: Mutex<CommandProcessState>,
    notify: Notify,
    output_observer: Option<Arc<dyn CommandOutputObserver>>,
    /// Wakes the operation's single capture writer task when an accepted chunk or a state change may
    /// give it work.
    ///
    /// stdout and stderr readers only accept chunks into the shared plan and notify this; the one
    /// writer task drains the plan to the backend, so both streams keep accepting up to the shared
    /// budget without ever waiting on disk. `notify_one` keeps a permit when the writer is between
    /// drains, so an accepted chunk is never silently dropped.
    capture_wake: Notify,
    /// Asks the lifecycle to terminate the process after a hard output failure.
    ///
    /// The read tasks hold a clone, so a capture that can no longer continue stops the process tree
    /// immediately instead of leaving it running while its output is silently dropped.
    output_failure: CancellationToken,
}

impl std::fmt::Debug for CommandProcessEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandProcessEntry")
            .field("process_id", &self.process_id)
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
    /// Durable capture bytes written so far; stdout and stderr share this operation budget.
    capture_bytes: u64,
    /// Length of the capture fragment the backend last confirmed writing.
    ///
    /// The prepared header before any chunk, then updated to the exact length each confirmed append
    /// reported. A repair truncates back to this fact, so it never depends on a fragment-length read
    /// that could fail and be mistaken for "nothing written".
    capture_committed_len: u64,
    /// Accepted capture chunks the backend has not confirmed writing yet, in acceptance order.
    ///
    /// Both streams push here through one plan, so a chunk one reader already accepted survives the
    /// other reader's failed append instead of being overwritten or appended past the fault. Bounded
    /// by [`MAX_CAPTURE_BYTES`]: each entry already counted against the shared budget.
    capture_pending: VecDeque<CaptureRepairChunk>,
    /// Whether a capture write already failed, so no further chunk may be appended.
    capture_write_failed: bool,
    /// Whether the operation's single capture writer has drained the plan and stopped.
    ///
    /// The operation may only publish its terminal result once this is set: the writer still draining
    /// the accepted plan is what turns "the process exited" into "every accepted byte is written or
    /// owed as one repair", so a snapshot taken before it settles could report a success that has not
    /// captured the bytes the observers already saw.
    capture_drained: bool,
    /// Exact reason a hard output capture failure terminated this operation, if any.
    output_failure: Option<CommandCaptureFailure>,
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
    /// Typed reason the durable capture could not continue, when a hard output failure ended it.
    ///
    /// `None` for a normal exit; a producer maps the category to its own storage boundary without
    /// parsing [`CommandOutputSnapshot::message`].
    pub output_failure: Option<CommandCaptureFailure>,
    /// Accepted capture chunks a failed capture did not store, in acceptance order.
    ///
    /// Derived from the operation's one capture plan when the snapshot is taken, so a retry replays
    /// every accepted chunk (not only the ones the first failing append saw).
    capture_plan: Option<CaptureRepair>,
    /// Length of the capture fragment the backend last confirmed writing.
    ///
    /// The accepted bytes this operation really holds are this length plus every chunk still owed, so
    /// a reported size never depends on a fragment-length read that could fail and understate it.
    pub capture_committed_len: u64,
}

impl CommandOutputSnapshot {
    /// The accepted bytes a failed capture append could not write, when the failure left a repair.
    ///
    /// A write failure that reached the capture file part-way keeps the offset the backend last
    /// confirmed and every accepted chunk it did not store, so a producer re-materializes those bytes
    /// before it archives anything. `None` means every accepted chunk is already on disk, so no repair
    /// is owed.
    pub fn capture_repair(&self) -> Option<&CaptureRepair> {
        self.capture_plan.as_ref()
    }
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
    /// Reports a hard capture failure the moment the reader observes it.
    ///
    /// The reader calls this under the failing operation's own boundary, *before* the process tree
    /// finishes terminating and draining, so an observer that holds the running call's channel can
    /// latch the typed fault while the call is still in flight instead of only when `execute`
    /// unwinds. The default is a no-op so a preview-only observer need not care.
    fn output_failed(&self, _failure: &CommandCaptureFailure) {}
}

#[derive(Debug, Default)]
struct ManagerLifetime(CancellationToken);

impl Drop for ManagerLifetime {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

impl<B> CommandProcessManager<B>
where
    B: CommandBackend,
{
    pub fn new(backend: Arc<B>) -> Self {
        Self::with_max_processes(backend, DEFAULT_MAX_PROCESSES)
    }

    /// The host backend this manager drives, shared so a repair can re-materialize accepted bytes.
    ///
    /// A caller that holds an accepted capture fragment a failed append could not write reaches the
    /// same backend through this handle, so the repair writes through the one backend that owns the
    /// capture path instead of a parallel copy.
    pub fn backend(&self) -> Arc<B> {
        self.backend.clone()
    }

    pub fn with_max_processes(backend: Arc<B>, max_processes: usize) -> Self {
        Self {
            state: Arc::new(Mutex::new(CommandProcessManagerState {
                entries: HashMap::new(),
                starting: 0,
                task_reservations: HashSet::new(),
                max_processes,
            })),
            backend,
            lifetime: Arc::new(ManagerLifetime::default()),
        }
    }

    pub async fn start(
        &self,
        request: CommandStartRequest,
    ) -> Result<CommandOutputSnapshot, PureError> {
        let yield_time = request.yield_time;
        let max_output_chars = request.max_output_chars;
        let entry = self.start_entry(request, None).await?;
        self.snapshot_after_wait(&entry.process_id, yield_time, max_output_chars)
            .await
    }

    /// Runs a command under its session task ID until process exit and output drain.
    ///
    /// # Errors
    /// Returns spawn, capture, identity, or output publication failures. Cancellation
    /// is passed to the process lifecycle and does not abandon the process waiter.
    pub async fn run_task(
        &self,
        task_id: &str,
        request: CommandStartRequest,
    ) -> Result<CommandOutputSnapshot, PureError> {
        let max_output_chars = request.max_output_chars;
        let entry = self.start_entry(request, Some(task_id)).await?;
        loop {
            let notified = entry.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if entry.is_final().await {
                break;
            }
            notified.await;
        }
        self.snapshot_entry(&entry, max_output_chars).await
    }

    async fn start_entry(
        &self,
        request: CommandStartRequest,
        task_id: Option<&str>,
    ) -> Result<Arc<CommandProcessEntry>, PureError> {
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
        let capture_committed_len = self
            .backend
            .prepare_output(&output_target, &request.command, &working_directory)
            .await
            .map_err(|error| tool_error("exec", error))?;

        if request
            .cancellation_token
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
        {
            return Err(tool_error("exec", "Task cancelled before process creation"));
        }
        let process_id = match task_id {
            Some(id) => {
                self.reserve_task_process(id).await?;
                id.to_owned()
            }
            None => self.reserve_process_id().await?,
        };
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
                self.state
                    .lock()
                    .await
                    .task_reservations
                    .remove(&process_id);
                return Err(tool_error("exec", error));
            }
        };
        let stdout = child.take_stdout();
        let stderr = child.take_stderr();
        let stdout_open = stdout.is_some();
        let stderr_open = stderr.is_some();
        let stdin = child.take_stdin();
        let output_failure = CancellationToken::new();
        let entry = Arc::new(CommandProcessEntry {
            process_id: process_id.clone(),
            output_target,
            stdin: Mutex::new(stdin),
            state: Mutex::new(CommandProcessState::new(
                stdout_open,
                stderr_open,
                capture_committed_len,
            )),
            notify: Notify::new(),
            capture_wake: Notify::new(),
            output_observer: request.output_observer,
            output_failure: output_failure.clone(),
        });
        {
            let mut state = self.state.lock().await;
            state.starting = state.starting.saturating_sub(1);
            state.task_reservations.remove(&process_id);
            state.entries.insert(process_id.clone(), entry.clone());
        }
        spawn_lifecycle_task(
            entry.clone(),
            child,
            CommandLifecycle {
                timeout: request.timeout,
                task_cancellation: request.cancellation_token,
                manager_cancellation: self.lifetime.0.clone(),
                output_failure,
            },
        );

        // The single capture writer owns the shared fragment. It is spawned here, before the readers,
        // so it is already waiting on `capture_wake` when the first chunk is accepted; a wake-up that
        // races the spawn is still held as a permit, so no accepted chunk is lost.
        tokio::spawn(run_capture_writer(entry.clone(), self.backend.clone()));
        if let Some(stdout) = stdout {
            tokio::spawn(read_stdout(entry.clone(), stdout));
        }
        if let Some(stderr) = stderr {
            tokio::spawn(read_stderr(entry.clone(), stderr));
        }
        Ok(entry)
    }

    pub async fn write_stdin(
        &self,
        request: CommandWriteRequest,
    ) -> Result<CommandOutputSnapshot, PureError> {
        let Some(entry) = self.entry(&request.process_id).await else {
            return Err(tool_error(
                "write_stdin",
                format!(
                    "task '{}' has no live process. Inspect get_tool_task before starting another command.",
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
                        "task '{}' does not accept stdin. Use wait for completion.",
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
                    "background process limit reached ({}). Use wait for an existing task to finish before starting another command.",
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

    async fn reserve_task_process(&self, id: &str) -> Result<(), PureError> {
        if !id.starts_with("task-") || id.len() > 256 {
            return Err(tool_error("exec", "invalid session task ID"));
        }
        let mut state = self.state.lock().await;
        if state.entries.contains_key(id) || state.task_reservations.contains(id) {
            return Err(tool_error("exec", "session task already owns a process"));
        }
        if state.entries.len().saturating_add(state.starting) >= state.max_processes {
            return Err(tool_error(
                "exec",
                "process capacity reached; wait for an existing task",
            ));
        }
        state.starting += 1;
        state.task_reservations.insert(id.to_owned());
        Ok(())
    }

    async fn release_start_reservation(&self) {
        let mut state = self.state.lock().await;
        state.starting = state.starting.saturating_sub(1);
    }

    async fn entry(&self, process_id: &str) -> Option<Arc<CommandProcessEntry>> {
        self.state.lock().await.entries.get(process_id).cloned()
    }

    /// Reads a live operation's current snapshot without consuming it.
    ///
    /// The producer path uses this the moment a capture failure is observed, so it can build the
    /// typed storage fault — and its retriable obligation — from the operation's stable capture path
    /// before the process tree has finished draining. `None` once the operation is no longer known.
    pub async fn snapshot(
        &self,
        process_id: &str,
        max_output_chars: usize,
    ) -> Option<CommandOutputSnapshot> {
        let entry = self.entry(process_id).await?;
        self.snapshot_entry(&entry, max_output_chars).await.ok()
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
        self.snapshot_entry(&entry, max_output_chars).await
    }

    async fn snapshot_entry(
        &self,
        entry: &CommandProcessEntry,
        max_output_chars: usize,
    ) -> Result<CommandOutputSnapshot, PureError> {
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
            self.state.lock().await.entries.remove(&entry.process_id);
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
        let (stdout, stderr) = if state.lifecycle.is_final() {
            (state.stdout.display_text(), state.stderr.display_text())
        } else {
            (
                state.pending_stdout.take_display_text(),
                state.pending_stderr.take_display_text(),
            )
        };
        let stdout = truncate_text(&stdout, max_output_chars);
        let stderr = truncate_text(&stderr, max_output_chars);
        let capture_plan = state.capture_repair();
        let capture_committed_len = state.capture_committed_len;
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
                output_failure: state.output_failure.clone(),
                capture_plan,
                capture_committed_len,
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
