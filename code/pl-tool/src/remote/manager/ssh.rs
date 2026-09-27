//! OpenSSH argv 与一次性命令边界。
//!
//! 连接参数（端口、用户、私钥、代理）由 `~/.ssh/config` 的 Host 别名解析，
//! 这里只传别名本身；BatchMode 固定关闭交互认证与提示。

use process_wrap::tokio::{ChildWrapper, CommandWrap};
use tokio::process::Command;

use super::SshServerProfile;
use crate::remote::RemoteClientError;
use pl_remote_helper::process::wrap_parent_bound_command;

pub(super) struct PreparedSshCommand {
    pub(super) command: CommandWrap,
}

pub(super) fn validate_profile(profile: &SshServerProfile) -> Result<(), RemoteClientError> {
    super::super::ssh_config::validate_alias(&profile.alias)?;
    for (field, value) in [
        ("hostName", profile.host_name.as_str()),
        ("username", profile.username.as_str()),
    ] {
        if value.trim().is_empty()
            || value.chars().any(char::is_control)
            || (matches!(field, "hostName" | "username") && value.starts_with('-'))
        {
            return Err(RemoteClientError::Protocol(format!(
                "SSH server {field} is invalid"
            )));
        }
    }
    if profile.port == 0 {
        return Err(RemoteClientError::Protocol(
            "SSH server port must be greater than zero".to_string(),
        ));
    }
    Ok(())
}

pub(super) async fn ssh_command(
    profile: &SshServerProfile,
    ssh_config: &super::super::ssh_config::SshConfigFile,
) -> Result<PreparedSshCommand, RemoteClientError> {
    let mut command = Command::new("ssh");
    command
        .arg("-T")
        // SSH 进程只承载 stdio 协议，不需要 X11；显式关闭可避免用户 ssh
        // 配置中的 ForwardX11 设置向远端注入图形会话并产生 xauth 警告。
        .arg("-x")
        .args([
            "-o",
            "ConnectTimeout=15",
            "-o",
            "ServerAliveInterval=15",
            "-o",
            "ServerAliveCountMax=3",
            "-o",
            "BatchMode=yes",
        ]);
    if ssh_config.is_explicit() {
        command.arg("-F").arg(ssh_config.path());
    }
    command.arg("--").arg(&profile.alias);
    Ok(PreparedSshCommand {
        command: wrap_parent_bound_command(command),
    })
}

/// OpenSSH invokes the account shell; all bootstrap syntax belongs to POSIX sh.
pub(super) fn posix_remote_command(script: &str) -> String {
    format!("exec /bin/sh -c '{}'", script.replace('\'', "'\\''"))
}

pub(super) async fn run_ssh_capture(
    profile: &SshServerProfile,
    ssh_config: &super::super::ssh_config::SshConfigFile,
    remote_command: &str,
) -> Result<String, RemoteClientError> {
    let mut prepared = ssh_command(profile, ssh_config).await?;
    prepared
        .command
        .command_mut()
        .arg(posix_remote_command(remote_command));
    let output = run_bounded_ssh(
        &mut prepared.command,
        None,
        "probe",
        std::time::Duration::from_secs(30),
    )
    .await?;
    if !output.status.success() {
        return Err(RemoteClientError::Protocol(format!(
            "ssh command failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    String::from_utf8(output.stdout)
        .map_err(|error| RemoteClientError::Protocol(format!("ssh output is not UTF-8: {error}")))
}

/// Owns the child through bounded timeout cleanup. Dropping the caller also
/// requests termination through the process-tree wrapper.
pub(super) async fn run_bounded_ssh(
    command: &mut CommandWrap,
    input: Option<&[u8]>,
    stage: &'static str,
    timeout: std::time::Duration,
) -> Result<std::process::Output, RemoteClientError> {
    use tokio::io::AsyncWriteExt;
    command
        .command_mut()
        .stdin(if input.is_some() {
            std::process::Stdio::piped()
        } else {
            std::process::Stdio::null()
        })
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = command.spawn().map_err(|error| {
        RemoteClientError::Protocol(format!("SSH {stage} spawn failed: {error}"))
    })?;
    let stdout = child.stdout().take();
    let stderr = child.stderr().take();
    let stdin = child.stdin().take();
    let operation = async {
        let write = async {
            if let (Some(mut stdin), Some(bytes)) = (stdin, input) {
                stdin.write_all(bytes).await?;
                stdin.shutdown().await?;
            }
            Ok::<_, std::io::Error>(())
        };
        let (status, stdout, stderr, ()) = tokio::try_join!(
            child.wait(),
            capture_bounded(stdout, 65536),
            capture_bounded(stderr, 4096),
            write
        )?;
        Ok::<_, std::io::Error>(std::process::Output {
            status,
            stdout,
            stderr,
        })
    };
    match tokio::time::timeout(timeout, operation).await {
        Ok(Ok(output)) => Ok(output),
        result => {
            let reason = match result {
                Ok(Err(error)) => error.to_string(),
                Err(_) => "timed out".into(),
                Ok(Ok(_)) => unreachable!(),
            };
            terminate_ssh_child(&mut child, stage)
                .await
                .map_err(|error| {
                    RemoteClientError::Protocol(format!(
                        "SSH {stage} cleanup failed after {reason}: {error}"
                    ))
                })?;
            Err(RemoteClientError::Protocol(format!("SSH {stage} {reason}")))
        }
    }
}

/// Requests process-tree termination and bounds the local reap wait. The caller
/// retains the wrapper until this attempt finishes or reports a cleanup error.
pub(super) async fn terminate_ssh_child(
    child: &mut Box<dyn ChildWrapper>,
    stage: &str,
) -> Result<(), RemoteClientError> {
    let termination = child.start_kill();
    tokio::time::timeout(std::time::Duration::from_secs(2), child.wait())
        .await
        .map_err(|_| RemoteClientError::Protocol(format!("SSH {stage} cleanup timed out")))?
        .map_err(|error| {
            RemoteClientError::Protocol(format!("failed to reap SSH {stage}: {error}"))
        })?;
    if let Err(error) = termination {
        tracing::debug!(%stage, %error, "SSH termination request failed, but the process exited");
    }
    Ok(())
}

async fn capture_bounded<R: tokio::io::AsyncRead + Unpin>(
    reader: Option<R>,
    limit: usize,
) -> std::io::Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    let Some(mut reader) = reader else {
        return Ok(Vec::new());
    };
    let mut retained = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        let count = reader.read(&mut buffer).await?;
        if count == 0 {
            return Ok(retained);
        }
        retained.extend_from_slice(&buffer[..count.min(limit.saturating_sub(retained.len()))]);
    }
}

pub(super) async fn initialization_diagnostic(
    stderr: Option<tokio::process::ChildStderr>,
) -> Result<String, RemoteClientError> {
    use tokio::io::AsyncReadExt;
    let Some(stderr) = stderr else {
        return Ok(String::new());
    };
    let mut bytes = Vec::new();
    stderr.take(4096).read_to_end(&mut bytes).await?;
    Ok(String::from_utf8_lossy(&bytes).trim().to_string())
}
