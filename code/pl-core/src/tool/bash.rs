use std::path::{Path, PathBuf};
use std::time::Duration;

use pl_protocol::PureError;
use serde::{Deserialize, Serialize};

use super::command::{
    CommandOutputSnapshot, CommandProcessManager, CommandStartRequest, CommandWriteRequest,
};
use super::truncation::{OutputTruncation, TruncationStrategy};
use super::{Tool, ToolContext, ToolInput, ToolOutput};

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

#[derive(Debug, Serialize)]
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

    fn tool_error(&self, msg: impl std::fmt::Display) -> PureError {
        PureError::ToolExecutionFailed {
            tool: self.name().to_string(),
            error: msg.to_string(),
        }
    }

    fn resolve_working_directory(
        &self,
        working_directory: Option<&Path>,
        allow_workspace_escape: bool,
    ) -> Result<PathBuf, PureError> {
        let workspace_root = std::fs::canonicalize(&self.workspace_root).map_err(|error| {
            self.tool_error(format!("failed to resolve workspace root: {error}"))
        })?;
        let candidate = match working_directory {
            Some(dir) if dir.is_absolute() => dir.to_path_buf(),
            Some(dir) => workspace_root.join(dir),
            None => workspace_root.clone(),
        };
        let canonical = std::fs::canonicalize(&candidate).map_err(|error| {
            self.tool_error(format!(
                "failed to resolve working directory '{}': {error}",
                candidate.display()
            ))
        })?;
        if allow_workspace_escape || canonical.starts_with(&workspace_root) {
            Ok(canonical)
        } else {
            Err(self.tool_error(format!(
                "working directory '{}' is outside the workspace",
                candidate.display()
            )))
        }
    }

    fn default_max_output_chars(&self) -> usize {
        self.truncation
            .head_limit
            .saturating_add(self.truncation.tail_limit)
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
                })
                .await?;

            tool_output_from_snapshot(snapshot, self.name())
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

            tool_output_from_snapshot(snapshot, self.name())
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
    })
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

    fn test_context() -> ToolContext {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        ToolContext {
            event_tx,
            options: crate::turn::TurnOptions::default(),
            workspace_access: crate::tool::WorkspaceAccess::WorkspaceOnly,
            mode: crate::turn::CompileMode::Auto,
            workspace_root: std::env::temp_dir(),
            workspace_instructions: None,
            active_subagent: None,
            agent_control: crate::AgentControl::default(),
            parent_session: std::sync::Arc::new(crate::CoreSession::new()),
        }
    }

    fn sleep_then_echo_command() -> &'static str {
        if cfg!(target_os = "windows") {
            "powershell -NoProfile -EncodedCommand UwB0AGEAcgB0AC0AUwBsAGUAZQBwACAALQBNAGkAbABsAGkAcwBlAGMAbwBuAGQAcwAgADcAMAAwADsAIABXAHIAaQB0AGUALQBPAHUAdABwAHUAdAAgACcAZABvAG4AZQAnAA=="
        } else {
            "sleep 0.7; echo done"
        }
    }

    fn stdin_echo_command() -> &'static str {
        if cfg!(target_os = "windows") {
            "powershell -NoProfile -EncodedCommand JABsAGkAbgBlACAAPQAgAFsAQwBvAG4AcwBvAGwAZQBdADoAOgBJAG4ALgBSAGUAYQBkAEwAaQBuAGUAKAApADsAIABXAHIAaQB0AGUALQBPAHUAdABwAHUAdAAgACgAJwBnAG8AdAA6ACcAIAArACAAJABsAGkAbgBlACkA"
        } else {
            "read line; echo got:$line"
        }
    }

    #[tokio::test]
    async fn echoes_hello() {
        let tool = test_tool();
        let output = tool
            .execute(tool_input("echo hello", "s1", "t1"), test_context())
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&output.description).unwrap();

        assert_eq!(value["status"], "completed");
        assert!(!output.timed_out);
        assert!(!output.truncated.stdout.was_truncated);
        assert!(output.truncated.stdout.content.contains("hello"));
        assert_eq!(output.exit_code, Some(0));
    }

    #[tokio::test]
    async fn captures_stderr() {
        let tool = test_tool();
        let output = tool
            .execute(tool_input("echo err >&2", "s2", "t2"), test_context())
            .await
            .unwrap();

        assert!(output.truncated.stderr.content.contains("err"));
    }

    #[tokio::test]
    async fn exit_code_nonzero() {
        let tool = test_tool();
        let output = tool
            .execute(tool_input("exit 42", "s3", "t3"), test_context())
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&output.description).unwrap();

        assert_eq!(output.exit_code, Some(42));
        assert_eq!(value["status"], "failed");
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
                },
                test_context(),
            )
            .await;

        assert!(result.is_err());
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
                },
                test_context(),
            )
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&running.description).unwrap();

        assert_eq!(value["status"], "running", "{value}");
        let process_id = value["processId"].as_str().unwrap().to_string();

        let completed = stdin
            .execute(
                ToolInput {
                    arguments: serde_json::json!({
                        "processId": process_id,
                        "chars": "",
                        "yieldTimeMs": 3000,
                    }),
                    session_id: "long-session".to_string(),
                    tool_id: "poll-tool".to_string(),
                },
                test_context(),
            )
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&completed.description).unwrap();

        assert_eq!(value["status"], "completed");
        assert!(value["stdout"].as_str().unwrap().contains("done"));
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
                },
                test_context(),
            )
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&running.description).unwrap();
        assert_eq!(value["status"], "running", "{value}");
        let process_id = value["processId"].as_str().unwrap().to_string();

        let completed = stdin
            .execute(
                ToolInput {
                    arguments: serde_json::json!({
                        "processId": process_id,
                        "chars": "hello\n",
                        "yieldTimeMs": 3000,
                    }),
                    session_id: "stdin-session".to_string(),
                    tool_id: "stdin-write".to_string(),
                },
                test_context(),
            )
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&completed.description).unwrap();

        assert_eq!(value["status"], "completed");
        assert!(value["stdout"].as_str().unwrap().contains("got:hello"));
    }

    #[tokio::test]
    async fn timeout_terminates_background_process() {
        let tool = test_tool();
        let output = tool
            .execute(
                ToolInput {
                    arguments: serde_json::json!({
                        "command": sleep_then_echo_command(),
                        "timeoutSeconds": 0,
                        "yieldTimeMs": 1000,
                    }),
                    session_id: "timeout-session".to_string(),
                    tool_id: "timeout-tool".to_string(),
                },
                test_context(),
            )
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&output.description).unwrap();

        assert_eq!(value["status"], "timedOut");
        assert!(output.timed_out);
        assert!(value["processId"].is_null());
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
                },
                test_context(),
            )
            .await
            .unwrap();
        let first_value: serde_json::Value = serde_json::from_str(&first.description).unwrap();
        let process_id = first_value["processId"].as_str().unwrap().to_string();

        let second = bash
            .execute(
                ToolInput {
                    arguments: serde_json::json!({
                        "command": sleep_then_echo_command(),
                        "yieldTimeMs": 250,
                    }),
                    session_id: "limit-session".to_string(),
                    tool_id: "second-tool".to_string(),
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
        let command = if cfg!(target_os = "windows") {
            "for /L %i in (1,1,250) do @echo aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        } else {
            "printf '%*s' 5000 '' | tr ' ' a"
        };
        let output = tool
            .execute(
                ToolInput {
                    arguments: serde_json::json!({
                        "command": command,
                        "maxOutputChars": 100,
                    }),
                    session_id: "large-session".to_string(),
                    tool_id: "large-tool".to_string(),
                },
                test_context(),
            )
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&output.description).unwrap();
        let stdout = value["stdout"].as_str().unwrap();
        let file_content = tokio::fs::read_to_string(&output.output_file)
            .await
            .unwrap();

        assert!(stdout.len() < 5000);
        assert!(stdout.contains("omitted"));
        assert!(file_content.len() > stdout.len());
    }
}
