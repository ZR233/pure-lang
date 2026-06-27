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
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn tool_input(command: &str, session_id: &str, tool_id: &str) -> ToolInput {
        ToolInput {
            arguments: serde_json::json!({ "command": command }),
            session_id: session_id.to_string(),
            tool_id: tool_id.to_string(),
            revision_base: 0,
        }
    }

    fn test_tool() -> BashTool {
        let root = std::env::temp_dir().join(format!(
            "pure-test-tool-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        BashTool::new(root)
    }

    fn shared_tools() -> (BashTool, WriteStdinTool) {
        let manager = CommandProcessManager::default();
        let bash = test_tool().with_process_manager(manager.clone());
        let stdin = WriteStdinTool::new(manager);
        (bash, stdin)
    }

    fn command_json(output: &ToolOutput) -> CommandJsonOutput {
        serde_json::from_str(&output.description).unwrap()
    }

    fn test_context() -> ToolContext {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        test_context_with_sender(event_tx)
    }

    fn test_context_with_sender(event_tx: pl_trace::AgentEventSender) -> ToolContext {
        ToolContext {
            event_tx,
            options: crate::turn::TurnOptions::default(),
            workspace_access: crate::tool::WorkspaceAccess::WorkspaceOnly,
            mode: crate::turn::CompileMode::Auto,
            workspace_root: std::env::temp_dir(),
            workspace_instructions: None,
            instruction_snapshot: None,
            active_subagent: None,
            agent_control: crate::AgentControl::default(),
            lsp_runtime: None,
            parent_session: std::sync::Arc::new(crate::CoreSession::new()),
        }
    }

    fn sleep_then_echo_command() -> &'static str {
        if cfg!(target_os = "windows") {
            "Start-Sleep -Milliseconds 700; Write-Output 'done'"
        } else {
            "sleep 0.7; echo done"
        }
    }

    fn long_sleep_then_echo_command() -> &'static str {
        if cfg!(target_os = "windows") {
            "Start-Sleep -Seconds 10; Write-Output 'done'"
        } else {
            "sleep 10; echo done"
        }
    }

    fn stdin_echo_command() -> &'static str {
        if cfg!(target_os = "windows") {
            "$line = [Console]::In.ReadLine(); Write-Output ('got:' + $line)"
        } else {
            "read line; echo got:$line"
        }
    }

    fn stderr_command() -> &'static str {
        if cfg!(target_os = "windows") {
            "Write-Error 'err'"
        } else {
            "echo err >&2"
        }
    }

    fn collect_tool_result_stream(
        event_rx: &mut tokio::sync::broadcast::Receiver<pl_trace::AgentEvent>,
    ) -> (String, u64) {
        let mut streamed = String::new();
        let mut revision = 0;
        while let Ok(event) = event_rx.try_recv() {
            if let pl_trace::AgentEvent::TracePartDelta { event } = event
                && let pl_trace::TraceDelta::ToolResult { delta } = event.delta
            {
                revision = revision.max(event.revision);
                streamed.push_str(&delta);
            }
        }
        (streamed, revision)
    }

    fn large_output_command() -> &'static str {
        if cfg!(target_os = "windows") {
            "for ($i = 0; $i -lt 250; $i++) { Write-Output 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' }"
        } else {
            "printf '%*s' 5000 '' | tr ' ' a"
        }
    }

    #[tokio::test]
    async fn echoes_hello() {
        let tool = test_tool();
        let output = tool
            .execute(tool_input("echo hello", "s1", "t1"), test_context())
            .await
            .unwrap();
        let result = command_json(&output);

        assert_eq!(result.status, "completed");
        assert!(!output.timed_out);
        assert!(!output.truncated.stdout.was_truncated);
        assert!(output.truncated.stdout.content.contains("hello"));
        assert_eq!(output.exit_code, Some(0));
    }

    #[tokio::test]
    async fn streams_tool_result_delta_for_command_output() {
        let tool = test_tool();
        let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(16);
        let mut input = tool_input("echo streaming", "stream-session", "stream-tool");
        input.revision_base = 5;
        let output = tool
            .execute(input, test_context_with_sender(event_tx))
            .await
            .unwrap();

        let (streamed, revision) = collect_tool_result_stream(&mut event_rx);

        assert!(streamed.contains("streaming"));
        assert!(revision > 5);
        assert!(output.runtime_events.iter().any(|event| matches!(
            event,
            ToolRuntimeEvent::ToolResultRevision {
                revision: output_revision
            } if *output_revision >= revision
        )));
    }

    #[tokio::test]
    async fn streams_tool_result_delta_for_stderr_output() {
        let tool = test_tool();
        let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(16);
        let output = tool
            .execute(
                tool_input(
                    stderr_command(),
                    "stderr-stream-session",
                    "stderr-stream-tool",
                ),
                test_context_with_sender(event_tx),
            )
            .await
            .unwrap();

        let (streamed, revision) = collect_tool_result_stream(&mut event_rx);

        assert!(streamed.contains("[stderr]"));
        assert!(streamed.contains("err"));
        assert!(output.runtime_events.iter().any(|event| matches!(
            event,
            ToolRuntimeEvent::ToolResultRevision {
                revision: output_revision
            } if *output_revision >= revision
        )));
    }

    #[tokio::test]
    async fn captures_stderr() {
        let tool = test_tool();
        let output = tool
            .execute(tool_input(stderr_command(), "s2", "t2"), test_context())
            .await
            .unwrap();

        assert!(output.truncated.stderr.content.contains("err"));
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_default_shell_executes_powershell_script() {
        let tool = test_tool();
        let output = tool
            .execute(
                tool_input(
                    "if ($PSVersionTable.PSVersion.Major -ge 5) { Write-Output 'powershell-ok' }; (Get-Location).Path",
                    "ps-session",
                    "ps-tool",
                ),
                test_context(),
            )
            .await
            .unwrap();

        assert_eq!(output.exit_code, Some(0));
        assert!(output.truncated.stdout.content.contains("powershell-ok"));
        assert!(output.truncated.stdout.content.lines().count() >= 2);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_powershell_captures_unicode_stdout() {
        let tool = test_tool();
        let output = tool
            .execute(
                tool_input("Write-Output '中文输出'", "unicode-session", "unicode-tool"),
                test_context(),
            )
            .await
            .unwrap();

        assert_eq!(output.exit_code, Some(0));
        assert!(output.truncated.stdout.content.contains("中文输出"));
    }

    #[tokio::test]
    async fn exit_code_nonzero() {
        let tool = test_tool();
        let output = tool
            .execute(tool_input("exit 42", "s3", "t3"), test_context())
            .await
            .unwrap();
        let result = command_json(&output);

        assert_eq!(output.exit_code, Some(42));
        assert_eq!(result.status, "failed");
    }

    #[tokio::test]
    async fn invalid_input_returns_error() {
        let tool = test_tool();
        let result = tool
            .execute(
                ToolInput {
                    arguments: serde_json::json!({}),
                    session_id: "s4".to_string(),
                    tool_id: "t4".to_string(),
                    revision_base: 0,
                },
                test_context(),
            )
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn defaults_to_workspace_root_as_current_directory() {
        let tool = test_tool();
        let output = tool
            .execute(
                tool_input("echo marker > cwd-check.txt", "cwd-session", "cwd-tool"),
                test_context(),
            )
            .await
            .unwrap();

        assert_eq!(output.exit_code, Some(0));
        assert!(tool.workspace_root.join("cwd-check.txt").exists());
        let _ = tokio::fs::remove_file(tool.workspace_root.join("cwd-check.txt")).await;
    }

    #[tokio::test]
    async fn rejects_working_directory_outside_workspace() {
        let tool = test_tool();
        let result = tool
            .execute(
                ToolInput {
                    arguments: serde_json::json!({
                        "command": "echo no",
                        "workingDirectory": ".."
                    }),
                    session_id: "cwd-session".to_string(),
                    tool_id: "escape-tool".to_string(),
                    revision_base: 0,
                },
                test_context(),
            )
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn relative_working_directory_resolves_from_workspace_root() {
        let tool = test_tool();
        tokio::fs::create_dir_all(tool.workspace_root.join("subdir"))
            .await
            .unwrap();
        let output = tool
            .execute(
                ToolInput {
                    arguments: serde_json::json!({
                        "command": "echo marker > cwd-check.txt",
                        "workingDirectory": "subdir",
                    }),
                    session_id: "cwd-session".to_string(),
                    tool_id: "relative-cwd".to_string(),
                    revision_base: 0,
                },
                test_context(),
            )
            .await
            .unwrap();

        assert_eq!(output.exit_code, Some(0));
        assert!(tool.workspace_root.join("subdir/cwd-check.txt").exists());
        let _ = tokio::fs::remove_dir_all(&tool.workspace_root).await;
    }

    #[tokio::test]
    async fn full_access_allows_working_directory_outside_workspace() {
        let tool = test_tool();
        let outside = tool.workspace_root.parent().unwrap().to_path_buf();
        let mut context = test_context();
        context.options = crate::turn::TurnOptions::default()
            .with_permission_mode(crate::turn::PermissionMode::FullAccess);
        let output = tool
            .execute(
                ToolInput {
                    arguments: serde_json::json!({
                        "command": "echo yes",
                        "workingDirectory": outside,
                    }),
                    session_id: "cwd-session".to_string(),
                    tool_id: "full-access-cwd".to_string(),
                    revision_base: 0,
                },
                context,
            )
            .await
            .unwrap();

        assert_eq!(output.exit_code, Some(0));
        let _ = tokio::fs::remove_file(&output.output_file).await;
        let _ =
            tokio::fs::remove_dir_all(output.output_file.parent().unwrap().parent().unwrap()).await;
    }

    #[tokio::test]
    async fn full_output_saved_to_file() {
        let tool = test_tool();
        let output = tool
            .execute(tool_input("echo test", "s5", "t5"), test_context())
            .await
            .unwrap();

        let content = tokio::fs::read_to_string(&output.output_file)
            .await
            .unwrap();
        assert!(content.contains("=== COMMAND ==="));
        assert!(content.contains("test"));

        let _ = tokio::fs::remove_file(&output.output_file).await;
        let _ = tokio::fs::remove_dir(output.output_file.parent().unwrap()).await;
        let _ = tokio::fs::remove_dir(output.output_file.parent().unwrap().parent().unwrap()).await;
    }

    #[tokio::test]
    async fn output_file_path_follows_convention() {
        let tool = test_tool();
        let output = tool
            .execute(
                tool_input("echo ok", "my-session", "my-tool"),
                test_context(),
            )
            .await
            .unwrap();

        let path = output.output_file;
        assert!(path.ends_with("target/pure/my-session/my-tool/output.log"));

        let _ = tokio::fs::remove_file(&path).await;
        let _ = tokio::fs::remove_dir_all(path.parent().unwrap().parent().unwrap()).await;
    }

    #[tokio::test]
    async fn long_command_returns_process_id_then_can_be_polled() {
        let (bash, stdin) = shared_tools();
        let running = bash
            .execute(
                ToolInput {
                    arguments: serde_json::json!({
                        "command": sleep_then_echo_command(),
                        "yieldTimeMs": 250,
                    }),
                    session_id: "long-session".to_string(),
                    tool_id: "long-tool".to_string(),
                    revision_base: 0,
                },
                test_context(),
            )
            .await
            .unwrap();
        let running_result = command_json(&running);

        assert_eq!(running_result.status, "running", "{running_result:?}");
        let process_id = running_result.process_id.unwrap();

        let mut result = None;
        for _ in 0..5 {
            let completed = stdin
                .execute(
                    ToolInput {
                        arguments: serde_json::json!({
                            "processId": process_id,
                            "chars": "",
                            "yieldTimeMs": 1500,
                        }),
                        session_id: "long-session".to_string(),
                        tool_id: "poll-tool".to_string(),
                        revision_base: 0,
                    },
                    test_context(),
                )
                .await
                .unwrap();
            result = Some(command_json(&completed));
            if result.as_ref().unwrap().status == "completed" {
                break;
            }
        }
        let result = result.unwrap();

        assert_eq!(result.status, "completed", "{result:?}");
        assert!(result.stdout.contains("done"));
    }

    #[tokio::test]
    async fn write_stdin_sends_input_to_running_process() {
        let (bash, stdin) = shared_tools();
        let running = bash
            .execute(
                ToolInput {
                    arguments: serde_json::json!({
                        "command": stdin_echo_command(),
                        "yieldTimeMs": 250,
                    }),
                    session_id: "stdin-session".to_string(),
                    tool_id: "stdin-tool".to_string(),
                    revision_base: 0,
                },
                test_context(),
            )
            .await
            .unwrap();
        let running_result = command_json(&running);
        assert_eq!(running_result.status, "running", "{running_result:?}");
        let process_id = running_result.process_id.unwrap();

        let mut result = None;
        for attempt in 0..5 {
            let completed = stdin
                .execute(
                    ToolInput {
                        arguments: serde_json::json!({
                            "processId": process_id,
                            "chars": if attempt == 0 { "hello\n" } else { "" },
                            "yieldTimeMs": 3000,
                        }),
                        session_id: "stdin-session".to_string(),
                        tool_id: format!("stdin-write-{attempt}"),
                        revision_base: 0,
                    },
                    test_context(),
                )
                .await
                .unwrap();
            result = Some(command_json(&completed));
            if result.as_ref().unwrap().status == "completed" {
                break;
            }
        }
        let result = result.unwrap();

        assert_eq!(result.status, "completed", "{result:?}");
        assert!(result.stdout.contains("got:hello"));
    }

    #[tokio::test]
    async fn timeout_terminates_background_process() {
        let (tool, stdin) = shared_tools();
        let output = tool
            .execute(
                ToolInput {
                    arguments: serde_json::json!({
                        "command": long_sleep_then_echo_command(),
                        "timeoutSeconds": 1,
                        "yieldTimeMs": 3000,
                    }),
                    session_id: "timeout-session".to_string(),
                    tool_id: "timeout-tool".to_string(),
                    revision_base: 0,
                },
                test_context(),
            )
            .await
            .unwrap();
        let mut result = command_json(&output);
        let mut timed_out = output.timed_out || result.timed_out;
        for attempt in 0..8 {
            if result.status == "timedOut" {
                break;
            }
            let Some(process_id) = result.process_id.clone() else {
                break;
            };
            let polled = stdin
                .execute(
                    ToolInput {
                        arguments: serde_json::json!({
                            "processId": process_id,
                            "yieldTimeMs": 1000,
                        }),
                        session_id: "timeout-session".to_string(),
                        tool_id: format!("timeout-poll-{attempt}"),
                        revision_base: 0,
                    },
                    test_context(),
                )
                .await
                .unwrap();
            timed_out = timed_out || polled.timed_out;
            result = command_json(&polled);
        }

        assert_eq!(result.status, "timedOut", "{result:?}");
        assert!(timed_out);
        assert_eq!(result.process_id, None);
    }

    #[tokio::test]
    async fn process_limit_returns_recoverable_error() {
        let manager = CommandProcessManager::new(1);
        let bash = test_tool().with_process_manager(manager.clone());
        let stdin = WriteStdinTool::new(manager);
        let first = bash
            .execute(
                ToolInput {
                    arguments: serde_json::json!({
                        "command": sleep_then_echo_command(),
                        "yieldTimeMs": 250,
                    }),
                    session_id: "limit-session".to_string(),
                    tool_id: "first-tool".to_string(),
                    revision_base: 0,
                },
                test_context(),
            )
            .await
            .unwrap();
        let process_id = command_json(&first).process_id.unwrap();

        let second = bash
            .execute(
                ToolInput {
                    arguments: serde_json::json!({
                        "command": sleep_then_echo_command(),
                        "yieldTimeMs": 250,
                    }),
                    session_id: "limit-session".to_string(),
                    tool_id: "second-tool".to_string(),
                    revision_base: 0,
                },
                test_context(),
            )
            .await;

        assert!(second.unwrap_err().to_string().contains("process limit"));

        let _ = stdin
            .execute(
                ToolInput {
                    arguments: serde_json::json!({
                        "processId": process_id,
                        "yieldTimeMs": 3000,
                    }),
                    session_id: "limit-session".to_string(),
                    tool_id: "cleanup-tool".to_string(),
                    revision_base: 0,
                },
                test_context(),
            )
            .await;
    }

    #[tokio::test]
    async fn write_stdin_unknown_process_is_recoverable_error() {
        let (_bash, stdin) = shared_tools();
        let result = stdin
            .execute(
                ToolInput {
                    arguments: serde_json::json!({ "processId": "missing" }),
                    session_id: "missing-session".to_string(),
                    tool_id: "missing-tool".to_string(),
                    revision_base: 0,
                },
                test_context(),
            )
            .await;

        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("not a live process")
        );
    }

    #[tokio::test]
    async fn large_output_is_truncated_in_json_and_saved_to_file() {
        let tool = test_tool();
        let output = tool
            .execute(
                ToolInput {
                    arguments: serde_json::json!({
                        "command": large_output_command(),
                        "maxOutputChars": 100,
                    }),
                    session_id: "large-session".to_string(),
                    tool_id: "large-tool".to_string(),
                    revision_base: 0,
                },
                test_context(),
            )
            .await
            .unwrap();
        let result = command_json(&output);
        let file_content = tokio::fs::read_to_string(&output.output_file)
            .await
            .unwrap();

        assert!(result.stdout.len() < 5000);
        assert!(result.stdout.contains("omitted"));
        assert!(file_content.len() > result.stdout.len());
    }
}
