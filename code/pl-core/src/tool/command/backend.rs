use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Stdio;

use pl_protocol::{PureError, Result};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};

use crate::process::{
    configure_background_command, terminate_process_tree, terminate_process_tree_sync,
};
use crate::tool::ToolPathPolicy;

use super::shell::command_for_environment;
use crate::execution_environment::ExecutionEnvironment;

/// 命令执行后端收到的启动请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpawnRequest {
    pub process_id: String,
    pub command: String,
    pub cwd: String,
    pub output_target: CommandOutputTarget,
}

pub type CommandReader = Pin<Box<dyn AsyncRead + Send>>;
pub type CommandWriter = Pin<Box<dyn AsyncWrite + Send>>;
type CommandWaitFuture =
    Pin<Box<dyn Future<Output = std::result::Result<CommandExit, String>> + Send>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandExit {
    pub exit_code: Option<i32>,
}

pub struct ManagedCommand {
    host_pid: Option<u32>,
    stdin: Option<CommandWriter>,
    stdout: Option<CommandReader>,
    stderr: Option<CommandReader>,
    wait: Option<CommandWaitFuture>,
}

impl std::fmt::Debug for ManagedCommand {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ManagedCommand")
            .field("host_pid", &self.host_pid)
            .field("stdin", &self.stdin.is_some())
            .field("stdout", &self.stdout.is_some())
            .field("stderr", &self.stderr.is_some())
            .finish_non_exhaustive()
    }
}

impl ManagedCommand {
    pub fn new(
        host_pid: Option<u32>,
        stdin: Option<CommandWriter>,
        stdout: Option<CommandReader>,
        stderr: Option<CommandReader>,
        wait: impl Future<Output = std::result::Result<CommandExit, String>> + Send + 'static,
    ) -> Self {
        Self {
            host_pid,
            stdin,
            stdout,
            stderr,
            wait: Some(Box::pin(wait)),
        }
    }

    pub fn host_pid(&self) -> Option<u32> {
        self.host_pid
    }

    pub fn take_stdin(&mut self) -> Option<CommandWriter> {
        self.stdin.take()
    }

    pub fn take_stdout(&mut self) -> Option<CommandReader> {
        self.stdout.take()
    }

    pub fn take_stderr(&mut self) -> Option<CommandReader> {
        self.stderr.take()
    }

    pub async fn wait(&mut self) -> std::result::Result<CommandExit, String> {
        let wait = self
            .wait
            .take()
            .ok_or_else(|| "managed command was already awaited".to_string())?;
        wait.await
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandCaptureStream {
    Stdout,
    Stderr,
}

/// 命令完整输出在宿主和模型 workspace 中的对应位置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutputTarget {
    capture_file: PathBuf,
    model_file: PathBuf,
    stdout_capture_file: Option<PathBuf>,
    stderr_capture_file: Option<PathBuf>,
}

impl CommandOutputTarget {
    pub fn new(capture_file: impl Into<PathBuf>, model_file: impl Into<PathBuf>) -> Self {
        Self {
            capture_file: capture_file.into(),
            model_file: model_file.into(),
            stdout_capture_file: None,
            stderr_capture_file: None,
        }
    }

    pub fn with_stream_capture_files(
        mut self,
        stdout: impl Into<PathBuf>,
        stderr: impl Into<PathBuf>,
    ) -> Self {
        self.stdout_capture_file = Some(stdout.into());
        self.stderr_capture_file = Some(stderr.into());
        self
    }

    pub fn capture_file(&self) -> &Path {
        &self.capture_file
    }

    pub fn model_file(&self) -> &Path {
        &self.model_file
    }

    pub fn stdout_capture_file(&self) -> Option<&Path> {
        self.stdout_capture_file.as_deref()
    }

    pub fn stderr_capture_file(&self) -> Option<&Path> {
        self.stderr_capture_file.as_deref()
    }
}

/// 计算模型可读取的统一命令完整输出路径。
pub fn command_output_model_path(session_id: &str, tool_id: &str) -> PathBuf {
    PathBuf::from("target")
        .join("pure")
        .join(safe_path_component(session_id, "session"))
        .join(safe_path_component(tool_id, "tool"))
        .join("output.log")
}

/// stdout/stderr 的累计原始字节数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandOutputSizes {
    pub stdout_bytes: u64,
    pub stderr_bytes: u64,
}

/// 为统一的 `exec` / `write_stdin` 工具提供环境相关能力。
///
/// PL 负责进程表、超时、stdin、输出截断和生命周期；实现只负责将命令映射到
/// 本地、容器或远程 workspace，并在结束时发布完整输出与 artifact 元数据。
pub trait CommandBackend: std::fmt::Debug + Send + Sync + 'static {
    type Error: std::fmt::Display + Send + 'static;

    fn resolve_cwd(
        &self,
        cwd: Option<&Path>,
        allow_workspace_escape: bool,
    ) -> impl std::future::Future<Output = std::result::Result<String, Self::Error>> + Send;

    fn output_target(
        &self,
        session_id: &str,
        tool_id: &str,
        call_id: &str,
        command: &str,
    ) -> impl std::future::Future<Output = std::result::Result<CommandOutputTarget, Self::Error>> + Send;

    fn spawn(
        &self,
        request: CommandSpawnRequest,
    ) -> impl std::future::Future<Output = std::result::Result<ManagedCommand, Self::Error>> + Send;

    fn prepare_output(
        &self,
        target: &CommandOutputTarget,
        command: &str,
        working_directory: &str,
    ) -> impl std::future::Future<Output = std::result::Result<(), Self::Error>> + Send;

    fn append_output_chunk(
        &self,
        target: &CommandOutputTarget,
        stream: CommandCaptureStream,
        chunk: &[u8],
    ) -> impl std::future::Future<Output = std::result::Result<(), Self::Error>> + Send;

    fn publish_output(
        &self,
        target: &CommandOutputTarget,
    ) -> impl std::future::Future<Output = std::result::Result<(), Self::Error>> + Send;

    fn collect_output_artifacts(
        &self,
        target: &CommandOutputTarget,
        sizes: CommandOutputSizes,
    ) -> impl std::future::Future<Output = std::result::Result<Vec<Value>, Self::Error>> + Send;

    fn terminate(
        &self,
        process_id: &str,
        host_pid: Option<u32>,
    ) -> impl std::future::Future<Output = ()> + Send;

    fn terminate_sync(&self, process_id: &str, host_pid: Option<u32>);
}

/// pure-studio 使用的本地 workspace 命令后端。
#[derive(Debug, Clone)]
pub struct LocalCommandBackend {
    workspace_root: PathBuf,
    execution_environment: ExecutionEnvironment,
}

impl LocalCommandBackend {
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
            execution_environment: ExecutionEnvironment::detect_local(),
        }
    }

    pub fn execution_environment(&self) -> &ExecutionEnvironment {
        &self.execution_environment
    }

    pub fn with_execution_environment(mut self, environment: ExecutionEnvironment) -> Self {
        self.execution_environment = environment;
        self
    }
}

impl CommandBackend for LocalCommandBackend {
    type Error = PureError;

    async fn resolve_cwd(
        &self,
        cwd: Option<&Path>,
        allow_workspace_escape: bool,
    ) -> Result<String> {
        let policy =
            ToolPathPolicy::new(self.workspace_root.clone(), allow_workspace_escape, "exec")?;
        match cwd {
            Some(dir) => policy.resolve_existing_directory(dir, &dir.display().to_string()),
            None => Ok(policy.root().to_path_buf()),
        }
        .map(|path| path.to_string_lossy().into_owned())
    }

    async fn output_target(
        &self,
        session_id: &str,
        tool_id: &str,
        _call_id: &str,
        _command: &str,
    ) -> Result<CommandOutputTarget> {
        let model_file = command_output_model_path(session_id, tool_id);
        Ok(CommandOutputTarget::new(
            self.workspace_root.join(&model_file),
            model_file,
        ))
    }

    async fn spawn(&self, request: CommandSpawnRequest) -> Result<ManagedCommand> {
        let mut command = command_for_environment(&self.execution_environment, &request.command);
        command.current_dir(&request.cwd);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        configure_background_command(&mut command);
        let mut child = command
            .spawn()
            .map_err(|error| command_error("exec", error))?;
        let host_pid = child.id();
        let stdin = child
            .stdin
            .take()
            .map(|value| Box::pin(value) as CommandWriter);
        let stdout = child
            .stdout
            .take()
            .map(|value| Box::pin(value) as CommandReader);
        let stderr = child
            .stderr
            .take()
            .map(|value| Box::pin(value) as CommandReader);
        Ok(ManagedCommand::new(
            host_pid,
            stdin,
            stdout,
            stderr,
            async move {
                child
                    .wait()
                    .await
                    .map(|status| CommandExit {
                        exit_code: status.code(),
                    })
                    .map_err(|error| format!("failed to wait for local process: {error}"))
            },
        ))
    }

    async fn prepare_output(
        &self,
        target: &CommandOutputTarget,
        command: &str,
        working_directory: &str,
    ) -> Result<()> {
        if let Some(parent) = target.capture_file().parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|error| {
                command_error(
                    "exec",
                    format!("failed to create output directory: {error}"),
                )
            })?;
        }
        let header = format!("=== COMMAND ===\n{command}\n\n=== CWD ===\n{working_directory}\n\n");
        tokio::fs::write(target.capture_file(), header.as_bytes())
            .await
            .map_err(|error| command_error("exec", format!("failed to write output file: {error}")))
    }

    async fn append_output_chunk(
        &self,
        target: &CommandOutputTarget,
        stream: CommandCaptureStream,
        chunk: &[u8],
    ) -> Result<()> {
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(target.capture_file())
            .await
            .map_err(|error| {
                command_error("exec", format!("failed to open output file: {error}"))
            })?;
        let label = match stream {
            CommandCaptureStream::Stdout => "STDOUT",
            CommandCaptureStream::Stderr => "STDERR",
        };
        file.write_all(format!("=== {label} ===\n").as_bytes())
            .await
            .map_err(|error| {
                command_error("exec", format!("failed to write output label: {error}"))
            })?;
        file.write_all(chunk).await.map_err(|error| {
            command_error("exec", format!("failed to write output chunk: {error}"))
        })?;
        if !chunk.ends_with(b"\n") {
            file.write_all(b"\n").await.map_err(|error| {
                command_error("exec", format!("failed to finish output chunk: {error}"))
            })?;
        }
        if let Some(stream_file) = match stream {
            CommandCaptureStream::Stdout => target.stdout_capture_file(),
            CommandCaptureStream::Stderr => target.stderr_capture_file(),
        } {
            let mut capture = tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(stream_file)
                .await
                .map_err(|error| {
                    command_error(
                        "exec",
                        format!("failed to open stream capture file: {error}"),
                    )
                })?;
            capture.write_all(chunk).await.map_err(|error| {
                command_error("exec", format!("failed to write stream capture: {error}"))
            })?;
        }
        Ok(())
    }

    async fn publish_output(&self, _target: &CommandOutputTarget) -> Result<()> {
        Ok(())
    }

    async fn collect_output_artifacts(
        &self,
        _target: &CommandOutputTarget,
        _sizes: CommandOutputSizes,
    ) -> Result<Vec<Value>> {
        Ok(Vec::new())
    }

    async fn terminate(&self, _process_id: &str, host_pid: Option<u32>) {
        terminate_process_tree(host_pid).await;
    }

    fn terminate_sync(&self, _process_id: &str, host_pid: Option<u32>) {
        terminate_process_tree_sync(host_pid);
    }
}

fn command_error(tool: &str, error: impl std::fmt::Display) -> PureError {
    PureError::ToolExecutionFailed {
        tool: tool.to_string(),
        error: error.to_string(),
    }
}

fn safe_path_component(value: &str, fallback: &str) -> String {
    let value = value
        .chars()
        .take(128)
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if value.is_empty() || matches!(value.as_str(), "." | "..") {
        fallback.to_string()
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_model_path_cannot_escape_workspace() {
        assert_eq!(
            command_output_model_path("../../session", "../tool"),
            PathBuf::from("target/pure/.._.._session/.._tool/output.log")
        );
        assert_eq!(
            command_output_model_path(".", ".."),
            PathBuf::from("target/pure/session/tool/output.log")
        );
    }
}
