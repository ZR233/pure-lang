use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt;
use pl_protocol::PureError;
use pl_trace::{AgentEvent, TracePartDeltaEvent};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::command::process_manager::*;
use super::command::{CommandBackend, LocalCommandBackend};
use super::truncation::{OutputTruncation, TruncationStrategy};
use super::{
    FunctionToolDefinition, Tool, ToolContext, ToolInput, ToolOutput, ToolRuntimeEvent,
    deserialize_tool_input,
};
use crate::time::unix_seconds;
use crate::turn::ToolEffect;

pub const TOOL_EXEC: &str = "exec";
pub const TOOL_WRITE_STDIN: &str = "write_stdin";

const DEFAULT_TIMEOUT_SECS: u64 = 60;
const DEFAULT_YIELD_TIME_MS: u64 = 10_000;
const MIN_YIELD_TIME_MS: u64 = 250;
const MAX_YIELD_TIME_MS: u64 = 30_000;
const MAX_MODEL_OUTPUT_CHARS: usize = 64 * 1024;

/// 启动命令并通过统一 workspace backend 执行的工具。
#[derive(Debug, Clone)]
pub struct ExecTool<B>
where
    B: CommandBackend,
{
    truncation: TruncationStrategy,
    default_timeout: Duration,
    process_manager: CommandProcessManager<B>,
}

/// 向 `exec` 启动的后台命令写入 stdin 或轮询输出。
#[derive(Debug, Clone)]
pub struct WriteStdinTool<B>
where
    B: CommandBackend,
{
    process_manager: CommandProcessManager<B>,
}

/// `exec` 的结构化输入。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecInput {
    /// The shell command to execute.
    pub command: String,
    /// Optional working directory relative to the agent workspace.
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    /// Optional total timeout in seconds (default: 60).
    #[serde(default)]
    #[schemars(range(min = 1))]
    pub timeout_seconds: Option<u64>,
    /// How long to wait before returning a running process id.
    #[serde(default)]
    pub yield_time_ms: Option<u64>,
    /// Maximum stdout/stderr characters returned to the model.
    #[serde(default)]
    #[schemars(range(min = 1))]
    pub max_output_chars: Option<usize>,
}

/// `write_stdin` 的结构化输入。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WriteStdinInput {
    /// Process id returned by a running exec result.
    pub process_id: String,
    /// Text to write; omit or pass an empty string to only wait or poll.
    #[serde(default)]
    pub chars: Option<String>,
    /// How long to wait for process exit; zero returns an immediate snapshot.
    #[serde(default)]
    pub yield_time_ms: Option<u64>,
    /// Maximum stdout/stderr characters returned to the model.
    #[serde(default)]
    #[schemars(range(min = 1))]
    pub max_output_chars: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommandJsonOutput {
    state: CommandProcessLifecycle,
    #[serde(skip_serializing_if = "Option::is_none")]
    process_id: Option<String>,
    stdout: String,
    stderr: String,
    output_file: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    output_artifacts: Vec<serde_json::Value>,
    message: String,
}

impl<B> ExecTool<B>
where
    B: CommandBackend,
{
    pub fn new(backend: Arc<B>) -> Self {
        Self {
            truncation: TruncationStrategy::default(),
            default_timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            process_manager: CommandProcessManager::new(backend),
        }
    }

    pub fn with_truncation(mut self, strategy: TruncationStrategy) -> Self {
        self.truncation = strategy;
        self
    }

    pub fn with_default_timeout(mut self, timeout: Duration) -> Self {
        self.default_timeout = timeout;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_process_manager(mut self, manager: CommandProcessManager<B>) -> Self {
        self.process_manager = manager;
        self
    }

    fn default_max_output_chars(&self) -> usize {
        self.truncation
            .head_limit
            .saturating_add(self.truncation.tail_limit)
    }
}

struct ToolResultOutputObserver {
    event_tx: tokio::sync::broadcast::WeakSender<AgentEvent>,
    turn_id: String,
    item_id: String,
    revision_base: u64,
}

impl CommandOutputObserver for ToolResultOutputObserver {
    fn output_chunk(&self, stream: CommandOutputStream, chunk: &[u8], revision: u64) {
        let mut delta = String::from_utf8_lossy(chunk).to_string();
        if matches!(stream, CommandOutputStream::Stderr) {
            delta = format!("[stderr] {delta}");
        }
        let now = unix_seconds();
        let event = TracePartDeltaEvent::running_tool_result(
            self.turn_id.clone(),
            self.item_id.clone(),
            self.revision_base.saturating_add(revision),
            now,
            delta,
        );
        if let Some(event_tx) = self.event_tx.upgrade() {
            let _ = event_tx.send(AgentEvent::TracePartDelta { event });
        }
    }
}

pub(crate) fn command_tool_pair<B>(backend: Arc<B>) -> (ExecTool<B>, WriteStdinTool<B>)
where
    B: CommandBackend,
{
    let manager = CommandProcessManager::new(backend);
    (
        ExecTool {
            truncation: TruncationStrategy::default(),
            default_timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            process_manager: manager.clone(),
        },
        WriteStdinTool::new(manager),
    )
}

pub(crate) fn local_command_tool_pair(
    workspace_root: PathBuf,
) -> (
    ExecTool<LocalCommandBackend>,
    WriteStdinTool<LocalCommandBackend>,
) {
    command_tool_pair(Arc::new(LocalCommandBackend::new(workspace_root)))
}

impl<B> WriteStdinTool<B>
where
    B: CommandBackend,
{
    pub(crate) fn new(process_manager: CommandProcessManager<B>) -> Self {
        Self { process_manager }
    }
}

impl<B> Tool for ExecTool<B>
where
    B: CommandBackend,
{
    fn name(&self) -> &str {
        TOOL_EXEC
    }

    fn description(&self) -> &str {
        "Start a shell command in the agent workspace and return a compact JSON result. If the command is still running after yieldTimeMs, use write_stdin with the returned processId. Full output is saved to a workspace-relative outputFile."
    }

    fn input_schema(&self) -> serde_json::Value {
        FunctionToolDefinition::<ExecInput>::new(self.name(), self.description()).input_schema()
    }

    fn effect(&self) -> Option<ToolEffect> {
        Some(ToolEffect::Process)
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> super::BoxFuture<'a, Result<ToolOutput, PureError>> {
        async move {
            let exec_input: ExecInput = deserialize_tool_input(self.name(), input.arguments)?;
            let timeout = exec_input
                .timeout_seconds
                .map(Duration::from_secs)
                .unwrap_or(self.default_timeout);
            let observer = Arc::new(ToolResultOutputObserver {
                event_tx: context.event_tx.downgrade(),
                turn_id: input.session_id.clone(),
                item_id: input.tool_id.clone(),
                revision_base: input.revision_base,
            });
            let call_id = context
                .provider_call_id
                .clone()
                .unwrap_or_else(|| input.tool_id.clone());
            let snapshot = self
                .process_manager
                .start(CommandStartRequest {
                    command: exec_input.command,
                    cwd: exec_input.cwd,
                    allow_workspace_escape: context.allows_workspace_escape(),
                    timeout,
                    yield_time: yield_duration(exec_input.yield_time_ms),
                    max_output_chars: max_output_chars(
                        exec_input.max_output_chars,
                        self.default_max_output_chars(),
                    ),
                    session_id: input.session_id,
                    tool_id: input.tool_id,
                    call_id,
                    cancellation_token: context.options.cancellation_token.clone(),
                    output_observer: Some(observer),
                })
                .await?;

            tool_output_from_snapshot(snapshot, self.name(), input.revision_base)
        }
        .boxed()
    }
}

impl<B> Tool for WriteStdinTool<B>
where
    B: CommandBackend,
{
    fn name(&self) -> &str {
        TOOL_WRITE_STDIN
    }

    fn description(&self) -> &str {
        "Write stdin to, or poll, a live process previously started by exec. Pass empty chars to wait without sending input. Does not start a new command or re-request command approval."
    }

    fn input_schema(&self) -> serde_json::Value {
        FunctionToolDefinition::<WriteStdinInput>::new(self.name(), self.description())
            .input_schema()
    }

    fn effect(&self) -> Option<ToolEffect> {
        Some(ToolEffect::Process)
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        _context: ToolContext,
    ) -> super::BoxFuture<'a, Result<ToolOutput, PureError>> {
        async move {
            let stdin_input: WriteStdinInput =
                deserialize_tool_input(self.name(), input.arguments)?;
            let chars = stdin_input.chars.unwrap_or_default();
            let yield_time = if chars.is_empty() {
                poll_yield_duration(stdin_input.yield_time_ms)
            } else {
                yield_duration(stdin_input.yield_time_ms)
            };
            let snapshot = self
                .process_manager
                .write_stdin(CommandWriteRequest {
                    process_id: stdin_input.process_id,
                    chars,
                    yield_time,
                    max_output_chars: max_output_chars(
                        stdin_input.max_output_chars,
                        TruncationStrategy::default()
                            .head_limit
                            .saturating_add(TruncationStrategy::default().tail_limit),
                    ),
                })
                .await?;

            tool_output_from_snapshot(snapshot, self.name(), input.revision_base)
        }
        .boxed()
    }
}

fn yield_duration(value: Option<u64>) -> Duration {
    let millis = match value {
        Some(0) => 0,
        Some(value) => value.clamp(MIN_YIELD_TIME_MS, MAX_YIELD_TIME_MS),
        None => DEFAULT_YIELD_TIME_MS,
    };
    Duration::from_millis(millis)
}

fn poll_yield_duration(value: Option<u64>) -> Duration {
    let millis = match value {
        Some(0) => 0,
        Some(value) => value.clamp(DEFAULT_YIELD_TIME_MS, MAX_YIELD_TIME_MS),
        None => DEFAULT_YIELD_TIME_MS,
    };
    Duration::from_millis(millis)
}

fn max_output_chars(value: Option<usize>, default: usize) -> usize {
    value.unwrap_or(default).clamp(1, MAX_MODEL_OUTPUT_CHARS)
}

fn tool_output_from_snapshot(
    snapshot: CommandOutputSnapshot,
    tool: &str,
    revision_base: u64,
) -> Result<ToolOutput, PureError> {
    let capture_file = snapshot.capture_file.clone();
    let exit_code = snapshot.state.exit_code();
    let timed_out = snapshot.state.is_timed_out();
    let output = CommandJsonOutput {
        state: snapshot.state,
        process_id: snapshot.process_id,
        stdout: snapshot.stdout.content.clone(),
        stderr: snapshot.stderr.content.clone(),
        output_file: snapshot.output_file.display().to_string(),
        output_artifacts: snapshot.output_artifacts.clone(),
        message: snapshot.message,
    };
    let description =
        serde_json::to_string(&output).map_err(|error| PureError::ToolExecutionFailed {
            tool: tool.to_string(),
            error: format!("failed to serialize command output: {error}"),
        })?;
    let mut runtime_events = vec![ToolRuntimeEvent::ToolResultRevision {
        revision: revision_base.saturating_add(snapshot.output_revision),
    }];
    if !snapshot.output_artifacts.is_empty() {
        runtime_events.push(ToolRuntimeEvent::OutputArtifacts {
            artifacts: snapshot.output_artifacts,
        });
    }
    Ok(ToolOutput {
        description,
        truncated: OutputTruncation {
            stdout: snapshot.stdout,
            stderr: snapshot.stderr,
        },
        output_file: capture_file,
        exit_code,
        timed_out,
        runtime_events,
    })
}

#[cfg(test)]
mod unit_tests;
