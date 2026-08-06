use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use pl_protocol::PureError;
use pl_trace::{AgentEvent, TraceDelta, TracePartDeltaEvent, TracePartKind, TracePartStatus};
use serde::{Deserialize, Serialize};

use super::command::process_manager::{
    CommandOutputObserver, CommandOutputSnapshot, CommandOutputStream, CommandProcessManager,
    CommandStartRequest, CommandWriteRequest,
};
use super::command::{CommandBackend, LocalCommandBackend};
use super::truncation::{OutputTruncation, TruncationStrategy};
use super::{Tool, ToolContext, ToolInput, ToolOutput, ToolRuntimeEvent};

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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecInput {
    pub command: String,
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub timeout_seconds: Option<u64>,
    #[serde(default)]
    pub yield_time_ms: Option<u64>,
    #[serde(default)]
    pub max_output_chars: Option<usize>,
}

/// `write_stdin` 的结构化输入。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WriteStdinInput {
    pub process_id: String,
    #[serde(default)]
    pub chars: Option<String>,
    #[serde(default)]
    pub yield_time_ms: Option<u64>,
    #[serde(default)]
    pub max_output_chars: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommandJsonOutput {
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    process_id: Option<String>,
    exit_code: Option<i32>,
    timed_out: bool,
    stdout: String,
    stderr: String,
    output_file: String,
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

    fn parse_input(arguments: serde_json::Value, tool_name: &str) -> Result<ExecInput, PureError> {
        serde_json::from_value(arguments).map_err(|error| PureError::ToolExecutionFailed {
            tool: tool_name.to_string(),
            error: format!("invalid input: {error}"),
        })
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
        let event = TracePartDeltaEvent {
            turn_id: self.turn_id.clone(),
            item_id: self.item_id.clone(),
            started_sequence: 0,
            revision: self.revision_base.saturating_add(revision),
            kind: TracePartKind::Tool,
            status: TracePartStatus::Running,
            created_at: now,
            updated_at: now,
            delta: TraceDelta::ToolResult { delta },
        };
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

    fn parse_input(
        arguments: serde_json::Value,
        tool_name: &str,
    ) -> Result<WriteStdinInput, PureError> {
        serde_json::from_value(arguments).map_err(|error| PureError::ToolExecutionFailed {
            tool: tool_name.to_string(),
            error: format!("invalid input: {error}"),
        })
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
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute"
                },
                "cwd": {
                    "type": "string",
                    "description": "Optional working directory relative to the agent workspace"
                },
                "timeoutSeconds": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Optional total timeout in seconds (default: 60)"
                },
                "yieldTimeMs": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "How long to wait before returning a running processId (default: 10000, clamped 250..30000 when non-zero)"
                },
                "maxOutputChars": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Maximum stdout/stderr chars in the JSON result; full output remains in outputFile"
                }
            },
            "required": ["command"],
            "additionalProperties": false
        })
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> super::BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            context.ensure_workspace_writable()?;
            let exec_input = Self::parse_input(input.arguments, self.name())?;
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
        })
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
        serde_json::json!({
            "type": "object",
            "properties": {
                "processId": {
                    "type": "string",
                    "description": "processId returned by a running exec result"
                },
                "chars": {
                    "type": "string",
                    "description": "Text to write to stdin. Omit or pass an empty string to only wait/poll."
                },
                "yieldTimeMs": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "How long to wait for output or process exit (default: 10000, clamped 250..30000 when non-zero)"
                },
                "maxOutputChars": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Maximum stdout/stderr chars in the JSON result; full output remains in outputFile"
                }
            },
            "required": ["processId"],
            "additionalProperties": false
        })
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        _context: ToolContext,
    ) -> super::BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            let stdin_input = Self::parse_input(input.arguments, self.name())?;
            let snapshot = self
                .process_manager
                .write_stdin(CommandWriteRequest {
                    process_id: stdin_input.process_id,
                    chars: stdin_input.chars.unwrap_or_default(),
                    yield_time: yield_duration(stdin_input.yield_time_ms),
                    max_output_chars: max_output_chars(
                        stdin_input.max_output_chars,
                        TruncationStrategy::default()
                            .head_limit
                            .saturating_add(TruncationStrategy::default().tail_limit),
                    ),
                })
                .await?;

            tool_output_from_snapshot(snapshot, self.name(), input.revision_base)
        })
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

fn max_output_chars(value: Option<usize>, default: usize) -> usize {
    value.unwrap_or(default).clamp(1, MAX_MODEL_OUTPUT_CHARS)
}

fn tool_output_from_snapshot(
    snapshot: CommandOutputSnapshot,
    tool: &str,
    revision_base: u64,
) -> Result<ToolOutput, PureError> {
    let capture_file = snapshot.capture_file.clone();
    let output = CommandJsonOutput {
        status: snapshot.status,
        process_id: snapshot.process_id,
        exit_code: snapshot.exit_code,
        timed_out: snapshot.timed_out,
        stdout: snapshot.stdout.content.clone(),
        stderr: snapshot.stderr.content.clone(),
        output_file: snapshot.output_file.display().to_string(),
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
        exit_code: snapshot.exit_code,
        timed_out: snapshot.timed_out,
        runtime_events,
    })
}

fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests;
