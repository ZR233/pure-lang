use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use pl_protocol::PureError;
use pl_trace::{AgentEvent, TraceDelta, TracePartDeltaEvent, TracePartKind, TracePartStatus};
use serde::{Deserialize, Serialize};

use super::command::{
    CommandOutputObserver, CommandOutputSnapshot, CommandOutputStream, CommandProcessManager,
    CommandStartRequest, CommandWriteRequest,
};
use super::truncation::{OutputTruncation, TruncationStrategy};
use super::{Tool, ToolContext, ToolInput, ToolOutput, ToolPathPolicy, ToolRuntimeEvent};

const TOOL_OUTPUT_DIR: &str = "target/pure";
const OUTPUT_LOG_FILE: &str = "output.log";
const DEFAULT_TIMEOUT_SECS: u64 = 60;
const DEFAULT_YIELD_TIME_MS: u64 = 10_000;
const MIN_YIELD_TIME_MS: u64 = 250;
const MAX_YIELD_TIME_MS: u64 = 30_000;
const MAX_MODEL_OUTPUT_CHARS: usize = 64 * 1024;

/// 执行 shell 命令并捕获输出的工具。
///
/// 短命令在当前工具调用内返回；长命令在 `yieldTimeMs` 后进入后台，
/// 返回 `processId`，由 `write_stdin` 继续观察或写入 stdin。
#[derive(Debug)]
pub struct BashTool {
    truncation: TruncationStrategy,
    workspace_root: PathBuf,
    default_timeout: Duration,
    process_manager: CommandProcessManager,
}

/// 向后台命令写入 stdin 或轮询输出的工具。
#[derive(Debug, Clone)]
pub struct WriteStdinTool {
    process_manager: CommandProcessManager,
}

/// BashTool 的结构化输入。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BashInput {
    pub command: String,
    #[serde(default)]
    pub working_directory: Option<PathBuf>,
    #[serde(default)]
    pub timeout_seconds: Option<u64>,
    #[serde(default)]
    pub yield_time_ms: Option<u64>,
    #[serde(default)]
    pub max_output_chars: Option<usize>,
}

/// WriteStdinTool 的结构化输入。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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

impl BashTool {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            truncation: TruncationStrategy::default(),
            workspace_root,
            default_timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            process_manager: CommandProcessManager::default(),
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

    pub(crate) fn with_process_manager(mut self, manager: CommandProcessManager) -> Self {
        self.process_manager = manager;
        self
    }

    fn output_path(&self, session_id: &str, tool_id: &str) -> PathBuf {
        self.workspace_root
            .join(TOOL_OUTPUT_DIR)
            .join(session_id)
            .join(tool_id)
            .join(OUTPUT_LOG_FILE)
    }

    fn parse_input(arguments: serde_json::Value, tool_name: &str) -> Result<BashInput, PureError> {
        serde_json::from_value(arguments).map_err(|error| PureError::ToolExecutionFailed {
            tool: tool_name.to_string(),
            error: format!("invalid input: {error}"),
        })
    }

    fn resolve_working_directory(
        &self,
        working_directory: Option<&Path>,
        allow_workspace_escape: bool,
    ) -> Result<PathBuf, PureError> {
        let policy = ToolPathPolicy::new(
            self.workspace_root.clone(),
            allow_workspace_escape,
            self.name(),
        )?;
        match working_directory {
            Some(dir) => policy.resolve_existing_directory(dir, &dir.display().to_string()),
            None => Ok(policy.root().to_path_buf()),
        }
    }

    fn default_max_output_chars(&self) -> usize {
        self.truncation
            .head_limit
            .saturating_add(self.truncation.tail_limit)
    }
}

struct ToolResultOutputObserver {
    event_tx: pl_trace::AgentEventSender,
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
        let _ = self.event_tx.send(AgentEvent::TracePartDelta { event });
    }
}

pub(crate) fn command_tool_pair(workspace_root: PathBuf) -> (BashTool, WriteStdinTool) {
    let manager = CommandProcessManager::default();
    (
        BashTool::new(workspace_root).with_process_manager(manager.clone()),
        WriteStdinTool::new(manager),
    )
}

impl WriteStdinTool {
    pub(crate) fn new(process_manager: CommandProcessManager) -> Self {
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

impl Tool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        "Start a shell command and return a compact JSON result. If the command \
         is still running after yieldTimeMs, the result includes processId; use \
         write_stdin with that processId to wait, poll, or send stdin. Full \
         stdout/stderr is saved to outputFile."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute"
                },
                "workingDirectory": {
                    "type": "string",
                    "description": "Optional working directory for the command"
                },
                "timeoutSeconds": {
                    "type": "integer",
                    "description": "Optional total timeout in seconds (default: 60)"
                },
                "yieldTimeMs": {
                    "type": "integer",
                    "description": "How long to wait before returning a running processId (default: 10000, clamped 250..30000)"
                },
                "maxOutputChars": {
                    "type": "integer",
                    "description": "Maximum stdout/stderr chars to include in the JSON result; full output remains in outputFile"
                }
            },
            "required": ["command"]
        })
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> super::BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            let bash_input = Self::parse_input(input.arguments, self.name())?;
            let timeout = bash_input
                .timeout_seconds
                .map(Duration::from_secs)
                .unwrap_or(self.default_timeout);
            let yield_time = yield_duration(bash_input.yield_time_ms);
            let max_output_chars =
                max_output_chars(bash_input.max_output_chars, self.default_max_output_chars());
            let working_directory = self.resolve_working_directory(
                bash_input.working_directory.as_deref(),
                context.allows_workspace_escape(),
            )?;
            let output_file = self.output_path(&input.session_id, &input.tool_id);
            let observer = Arc::new(ToolResultOutputObserver {
                event_tx: context.event_tx.clone(),
                turn_id: input.session_id.clone(),
                item_id: input.tool_id.clone(),
                revision_base: input.revision_base,
            });
            let snapshot = self
                .process_manager
                .start(CommandStartRequest {
                    command: bash_input.command,
                    working_directory,
                    timeout,
                    yield_time,
                    max_output_chars,
                    output_file,
                    cancellation_token: context.options.cancellation_token.clone(),
                    output_observer: Some(observer),
                })
                .await?;

            tool_output_from_snapshot(snapshot, self.name(), input.revision_base)
        })
    }
}

impl Tool for WriteStdinTool {
    fn name(&self) -> &str {
        "write_stdin"
    }

    fn description(&self) -> &str {
        "Write stdin to, or poll, a live process previously started by bash. \
         Pass empty chars to wait without sending input. Does not start a new \
         command or re-request command approval."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "processId": {
                    "type": "string",
                    "description": "processId returned by a running bash result"
                },
                "chars": {
                    "type": "string",
                    "description": "Text to write to stdin. Omit or pass an empty string to only wait/poll."
                },
                "yieldTimeMs": {
                    "type": "integer",
                    "description": "How long to wait for output or process exit (default: 10000, clamped 250..30000)"
                },
                "maxOutputChars": {
                    "type": "integer",
                    "description": "Maximum stdout/stderr chars to include in the JSON result; full output remains in outputFile"
                }
            },
            "required": ["processId"]
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
    Duration::from_millis(
        value
            .unwrap_or(DEFAULT_YIELD_TIME_MS)
            .clamp(MIN_YIELD_TIME_MS, MAX_YIELD_TIME_MS),
    )
}

fn max_output_chars(value: Option<usize>, default: usize) -> usize {
    value.unwrap_or(default).min(MAX_MODEL_OUTPUT_CHARS)
}

fn tool_output_from_snapshot(
    snapshot: CommandOutputSnapshot,
    tool: &str,
    revision_base: u64,
) -> Result<ToolOutput, PureError> {
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
    Ok(ToolOutput {
        description,
        truncated: OutputTruncation {
            stdout: snapshot.stdout,
            stderr: snapshot.stderr,
        },
        output_file: snapshot.output_file,
        exit_code: snapshot.exit_code,
        timed_out: snapshot.timed_out,
        runtime_events: vec![ToolRuntimeEvent::ToolResultRevision {
            revision: revision_base.saturating_add(snapshot.output_revision),
        }],
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
