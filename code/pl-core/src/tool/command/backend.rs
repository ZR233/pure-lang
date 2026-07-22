use std::path::{Path, PathBuf};
use std::process::Stdio;

use pl_protocol::{PureError, Result};
use serde_json::Value;
use tokio::process::Child;

use crate::process::{
    configure_background_command, terminate_process_tree, terminate_process_tree_sync,
};
use crate::tool::ToolPathPolicy;

use super::shell::shell_command;

/// 命令执行后端收到的启动请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpawnRequest {
    pub process_id: String,
    pub command: String,
    pub cwd: PathBuf,
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
    ) -> impl std::future::Future<Output = std::result::Result<PathBuf, Self::Error>> + Send;

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
    ) -> impl std::future::Future<Output = std::result::Result<Child, Self::Error>> + Send;

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
}

impl LocalCommandBackend {
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
        }
    }
}

impl CommandBackend for LocalCommandBackend {
    type Error = PureError;

    async fn resolve_cwd(
        &self,
        cwd: Option<&Path>,
        allow_workspace_escape: bool,
    ) -> Result<PathBuf> {
        let policy =
            ToolPathPolicy::new(self.workspace_root.clone(), allow_workspace_escape, "exec")?;
        match cwd {
            Some(dir) => policy.resolve_existing_directory(dir, &dir.display().to_string()),
            None => Ok(policy.root().to_path_buf()),
        }
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

    async fn spawn(&self, request: CommandSpawnRequest) -> Result<Child> {
        let mut command = shell_command(&request.command);
        command.current_dir(&request.cwd);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        configure_background_command(&mut command);
        command
            .spawn()
            .map_err(|error| command_error("exec", error))
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
