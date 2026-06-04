use std::path::{Path, PathBuf};
use std::time::Duration;

use pl_protocol::PureError;
use serde::{Deserialize, Serialize};
use tokio::process::Command;

use super::truncation::{OutputTruncation, TruncationStrategy};
use super::{Tool, ToolContext, ToolInput, ToolOutput};

const TOOL_OUTPUT_DIR: &str = "target/pure";
const OUTPUT_LOG_FILE: &str = "output.log";
const DEFAULT_TIMEOUT_SECS: u64 = 60;

/// 执行 shell 命令并捕获输出的工具。
///
/// 通过 `tokio::process::Command` 异步执行命令，分别收集 stdout 和 stderr，
/// 截断后用于内联展示，完整输出写入文件。
///
/// 平台行为：
/// - Windows: 通过 `cmd /C` 执行
/// - Unix: 通过 `sh -c` 执行
///
/// 超时：默认 60 秒，可通过 `BashInput::timeout_seconds` 覆盖。
#[derive(Debug)]
pub struct BashTool {
    truncation: TruncationStrategy,
    workspace_root: PathBuf,
    default_timeout: Duration,
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
}

impl BashTool {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            truncation: TruncationStrategy::default(),
            workspace_root,
            default_timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
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

    fn output_path(&self, session_id: &str, tool_id: &str) -> PathBuf {
        self.workspace_root
            .join(TOOL_OUTPUT_DIR)
            .join(session_id)
            .join(tool_id)
            .join(OUTPUT_LOG_FILE)
    }

    fn shell_command() -> (&'static str, &'static str) {
        if cfg!(target_os = "windows") {
            ("cmd", "/C")
        } else {
            ("sh", "-c")
        }
    }

    fn parse_input(arguments: serde_json::Value, tool_name: &str) -> Result<BashInput, PureError> {
        serde_json::from_value(arguments).map_err(|e| PureError::ToolExecutionFailed {
            tool: tool_name.to_string(),
            error: format!("invalid input: {e}"),
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
}

/// 当 `wait_with_output` 因超时被丢弃时，通过 PID 杀死已孤立的子进程。
fn kill_orphan_process(child_id: Option<u32>) {
    let Some(id) = child_id else { return };
    let result = if cfg!(unix) {
        std::process::Command::new("kill")
            .arg("-9")
            .arg(id.to_string())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
    } else {
        std::process::Command::new("taskkill")
            .args(["/F", "/PID", &id.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
    };
    // 尽力而为；如果进程已经退出，kill 会失败（无害）
    if let Ok(mut child) = result {
        let _ = child.wait();
    }
}

impl Tool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        "Execute a shell command. Output is truncated to first/last 1000 chars \
         with full output saved to a file. Use the output_file field to access \
         the complete output."
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
                    "description": "Optional timeout in seconds (default: 60)"
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

            let (shell, flag) = Self::shell_command();
            let mut command = Command::new(shell);
            command.args([flag, &bash_input.command]);

            let working_directory = self.resolve_working_directory(
                bash_input.working_directory.as_deref(),
                context.allows_workspace_escape(),
            )?;
            command.current_dir(working_directory);

            command.stdout(std::process::Stdio::piped());
            command.stderr(std::process::Stdio::piped());

            let child = command
                .spawn()
                .map_err(|e| self.tool_error(format!("failed to spawn command: {e}")))?;
            let child_id = child.id();

            let wait_for_output = async {
                match tokio::time::timeout(timeout, child.wait_with_output()).await {
                    Ok(Ok(output)) => Ok((
                        String::from_utf8_lossy(&output.stdout).to_string(),
                        String::from_utf8_lossy(&output.stderr).to_string(),
                        output.status.code(),
                        false,
                    )),
                    Ok(Err(e)) => Err(self.tool_error(format!("command execution failed: {e}"))),
                    Err(_elapsed) => {
                        kill_orphan_process(child_id);
                        Ok((
                            String::new(),
                            format!("Command timed out after {} seconds", timeout.as_secs()),
                            None,
                            true,
                        ))
                    }
                }
            };

            let (stdout, stderr, exit_code, timed_out) = match &context.options.cancellation_token {
                Some(token) => {
                    tokio::select! {
                        result = wait_for_output => result?,
                        _ = token.cancelled() => {
                            kill_orphan_process(child_id);
                            return Err(self.tool_error("command interrupted by user"));
                        }
                    }
                }
                None => wait_for_output.await?,
            };

            if context
                .options
                .cancellation_token
                .as_ref()
                .is_some_and(|token| token.is_cancelled())
            {
                kill_orphan_process(child_id);
                return Err(self.tool_error("command interrupted by user"));
            }

            let output_path = self.output_path(&input.session_id, &input.tool_id);
            if let Some(parent) = output_path.parent() {
                tokio::fs::create_dir_all(parent).await.map_err(|e| {
                    self.tool_error(format!("failed to create output directory: {e}"))
                })?;
            }

            {
                let combined = format!("=== STDOUT ===\n{stdout}\n\n=== STDERR ===\n{stderr}\n");
                tokio::fs::write(&output_path, combined.as_bytes())
                    .await
                    .map_err(|e| self.tool_error(format!("failed to write output file: {e}")))?;
            }

            let stdout_truncated = self.truncation.truncate(&stdout);
            let stderr_truncated = self.truncation.truncate(&stderr);

            let description = if timed_out {
                format!("Command timed out after {} seconds", timeout.as_secs())
            } else {
                match exit_code {
                    Some(0) => "Command exited successfully (code 0)".to_string(),
                    Some(code) => format!("Command exited with code {code}"),
                    None => "Command terminated (no exit code available)".to_string(),
                }
            };

            Ok(ToolOutput {
                description,
                truncated: OutputTruncation {
                    stdout: stdout_truncated,
                    stderr: stderr_truncated,
                },
                output_file: output_path,
                exit_code,
                timed_out,
            })
        })
    }
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
        let root = std::env::temp_dir().join("pure-test-tool");
        std::fs::create_dir_all(&root).unwrap();
        BashTool::new(root)
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
        }
    }

    #[tokio::test]
    async fn echoes_hello() {
        let tool = test_tool();
        let output = tool
            .execute(tool_input("echo hello", "s1", "t1"), test_context())
            .await
            .unwrap();

        assert_eq!(output.description, "Command exited successfully (code 0)");
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

        assert_eq!(output.exit_code, Some(42));
        assert!(output.description.contains("42"));
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
        assert!(content.contains("=== STDOUT ==="));
        assert!(content.contains("test"));

        // 清理
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

        // 清理
        let _ = tokio::fs::remove_file(&path).await;
        let _ = tokio::fs::remove_dir_all(path.parent().unwrap().parent().unwrap()).await;
    }
}
