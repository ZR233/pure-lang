//! LSP server driver：单个 server 生命周期的唯一 adapter 边界。
//!
//! 环境探测与修复、进程启动参数解析和 server 特殊初始化都由具体 driver 实现；
//! registry 与路由层只面向 [`LspServerDriver`]，不包含任何语言专项逻辑。

pub(crate) mod command;
pub(crate) mod rust_analyzer;

use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use futures::future::BoxFuture;
use serde_json::Value;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::catalog::LspServerDefinition;
use crate::process::spawn_background;
use crate::types::LspMissingComponent;

pub(crate) const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// 解析占位符后的进程启动命令。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspResolvedCommand {
    pub program: String,
    pub args: Vec<String>,
}

/// 环境探测的 typed 结果：就绪或带修复说明的缺失原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LspProbeOutcome {
    Ready { version: String },
    MissingCommand { message: String },
    MissingComponent(LspMissingComponent),
    Failed { message: String },
}

/// 修复缺失组件失败的原因。
#[derive(Debug, thiserror::Error)]
pub enum LspRepairError {
    #[error("server does not provide a repair action")]
    NotSupported,
    #[error("{tool} was not found on PATH")]
    MissingTool { tool: String },
    #[error("repair failed: {0}")]
    Failed(String),
}

/// 单个 LSP server 的生命周期 adapter。
///
/// catalog 需要运行期开放扩展（内置、用户配置与宿主自定义 driver 共存），
/// 因此以 `dyn` 分发；future 用 `BoxFuture`（与 pl-core `Tool` 相同），不使用
/// `#[async_trait]`。
pub trait LspServerDriver: Send + Sync {
    /// 进程启动参数解析：默认渲染 catalog command 的 `{workspaceRoot}` 占位符。
    fn resolve_command(
        &self,
        definition: &LspServerDefinition,
        workspace_root: &Path,
    ) -> LspResolvedCommand {
        definition.command.render(workspace_root)
    }

    /// 环境探测：返回 typed 就绪/缺失原因，不启动 server 进程。
    fn probe<'a>(&'a self, command: &'a LspResolvedCommand) -> BoxFuture<'a, LspProbeOutcome>;

    /// 修复探测发现的缺失组件；只在 `missingServerComponent` 状态下被调用。
    fn repair<'a>(
        &'a self,
        component: &'a LspMissingComponent,
    ) -> BoxFuture<'a, Result<(), LspRepairError>>;

    /// LSP initialize 请求的 initializationOptions（server 特殊初始化）。
    fn initialization_options(&self) -> Value {
        Value::Null
    }

    /// workspace/configuration 请求中单个 section 的响应值。
    fn configuration_response(&self, _section: Option<&str>) -> Value {
        Value::Null
    }
}

/// 命令执行的 typed 失败；`stderr` 供 driver 做组件缺失判定。
#[derive(Debug)]
pub(crate) enum CommandProbeError {
    MissingCommand,
    Failed { message: String, stderr: String },
}

/// 运行一次性命令并捕获输出；用于 driver 的探测与修复。
pub(crate) async fn run_command_capture(
    program: &str,
    args: &[&str],
    timeout: Duration,
    timeout_message: &str,
) -> Result<Vec<u8>, CommandProbeError> {
    let mut command_process = Command::new(program);
    command_process
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = spawn_background(command_process).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            CommandProbeError::MissingCommand
        } else {
            CommandProbeError::Failed {
                message: error.to_string(),
                stderr: String::new(),
            }
        }
    })?;
    let stdout = child.stdout().take();
    let stderr = child.stderr().take();
    let stdout_task = tokio::spawn(read_child_output(stdout));
    let stderr_task = tokio::spawn(read_child_output(stderr));
    let status = match tokio::time::timeout(timeout, child.wait()).await {
        Ok(Ok(status)) => status,
        Ok(Err(error)) => {
            return Err(CommandProbeError::Failed {
                message: error.to_string(),
                stderr: String::new(),
            });
        }
        Err(_) => {
            let kill = child.kill();
            let _ = std::pin::Pin::from(kill).await;
            return Err(CommandProbeError::Failed {
                message: timeout_message.to_string(),
                stderr: String::new(),
            });
        }
    };
    let stdout = stdout_task.await.unwrap_or_else(|error| {
        tracing::warn!("command stdout task failed: {error}");
        Vec::new()
    });
    let stderr = stderr_task.await.unwrap_or_default();
    if status.success() {
        return Ok(if stdout.is_empty() { stderr } else { stdout });
    }
    Err(CommandProbeError::Failed {
        message: command_failure_message(status, &stdout, &stderr),
        stderr: String::from_utf8_lossy(&stderr).to_string(),
    })
}

fn command_failure_message(status: ExitStatus, stdout: &[u8], stderr: &[u8]) -> String {
    let mut parts = Vec::new();
    parts.push(format!("command failed with {status}"));
    let stdout = String::from_utf8_lossy(stdout);
    if !stdout.is_empty() {
        parts.push(format!("stdout: {stdout}"));
    }
    let stderr = String::from_utf8_lossy(stderr);
    if !stderr.is_empty() {
        parts.push(format!("stderr: {stderr}"));
    }
    parts.join("\n")
}

async fn read_child_output(stream: Option<impl tokio::io::AsyncRead + Unpin>) -> Vec<u8> {
    let Some(mut stream) = stream else {
        return Vec::new();
    };
    let mut output = Vec::new();
    let _ = stream.read_to_end(&mut output).await;
    output
}
