use std::path::PathBuf;
use std::process::{ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::host::{LspHostBackend, LspHostSpawnRequest, spawn_background};

pub(crate) const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
static NEXT_HOST_PROBE_ID: AtomicU64 = AtomicU64::new(0);

/// 命令执行的 typed 失败；`stderr` 供 driver 做组件缺失判定。
#[derive(Debug)]
pub(crate) enum CommandProbeError {
    MissingCommand,
    Failed { message: String, stderr: String },
}

pub(crate) async fn run_command_capture(
    program: &str,
    args: &[&str],
    timeout: Duration,
    timeout_message: &str,
    host: Option<&dyn LspHostBackend>,
) -> Result<Vec<u8>, CommandProbeError> {
    if let Some(host) = host {
        return run_host_command_capture(host, program, args, timeout, timeout_message).await;
    }
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

async fn run_host_command_capture(
    host: &dyn LspHostBackend,
    program: &str,
    args: &[&str],
    timeout: Duration,
    timeout_message: &str,
) -> Result<Vec<u8>, CommandProbeError> {
    let sequence = NEXT_HOST_PROBE_ID
        .fetch_add(1, Ordering::Relaxed)
        .saturating_add(1);
    let mut child = host
        .spawn(LspHostSpawnRequest {
            process_id: format!("lsp-probe-{}-{sequence}", std::process::id()),
            program: program.to_string(),
            args: args.iter().map(|arg| (*arg).to_string()).collect(),
            cwd: PathBuf::from("."),
        })
        .await
        .map_err(|error| CommandProbeError::Failed {
            message: error.to_string(),
            stderr: String::new(),
        })?;
    let stdout_task = tokio::spawn(read_child_output(child.take_stdout()));
    let stderr_task = tokio::spawn(read_child_output(child.take_stderr()));
    let exit = match tokio::time::timeout(timeout, child.wait()).await {
        Ok(Ok(exit)) => exit,
        Ok(Err(error)) => {
            return Err(CommandProbeError::Failed {
                message: error.to_string(),
                stderr: String::new(),
            });
        }
        Err(_) => {
            child.terminate().await;
            return Err(CommandProbeError::Failed {
                message: timeout_message.to_string(),
                stderr: String::new(),
            });
        }
    };
    let stdout = stdout_task.await.unwrap_or_else(|error| {
        tracing::warn!("host command stdout task failed: {error}");
        Vec::new()
    });
    let stderr = stderr_task.await.unwrap_or_default();
    if exit.exit_code == Some(0) {
        return Ok(if stdout.is_empty() { stderr } else { stdout });
    }
    if exit.exit_code == Some(127) {
        return Err(CommandProbeError::MissingCommand);
    }
    Err(CommandProbeError::Failed {
        message: host_command_failure_message(exit.exit_code, &stdout, &stderr),
        stderr: String::from_utf8_lossy(&stderr).to_string(),
    })
}

fn host_command_failure_message(status: Option<i32>, stdout: &[u8], stderr: &[u8]) -> String {
    let mut parts = vec![format!(
        "command failed with exit code {}",
        status.map_or_else(|| "unknown".to_string(), |code| code.to_string())
    )];
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

fn command_failure_message(status: ExitStatus, stdout: &[u8], stderr: &[u8]) -> String {
    let mut parts = vec![format!("command failed with {status}")];
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
