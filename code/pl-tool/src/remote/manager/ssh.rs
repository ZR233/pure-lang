//! OpenSSH argv、Askpass lease 与一次性命令边界。

use tokio::process::Command;

use super::{SshAuth, SshServerProfile};
use crate::remote::RemoteClientError;
use pl_remote_helper::process::configure_background_command;

pub(super) struct PreparedSshCommand {
    pub(super) command: Command,
    pub(super) askpass: Option<tempfile::TempDir>,
}

pub(super) fn validate_profile(profile: &SshServerProfile) -> Result<(), RemoteClientError> {
    for (field, value) in [
        ("id", profile.id.as_str()),
        ("name", profile.name.as_str()),
        ("host", profile.host.as_str()),
        ("username", profile.username.as_str()),
    ] {
        if value.trim().is_empty()
            || value.chars().any(char::is_control)
            || (matches!(field, "host" | "username") && value.starts_with('-'))
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
    password: Option<&str>,
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
        ])
        .arg("-p")
        .arg(profile.port.to_string())
        .arg("-l")
        .arg(&profile.username);
    if let SshAuth::AgentOrKey {
        identity_file: Some(identity_file),
    } = &profile.auth
    {
        command.arg("-i").arg(identity_file);
    }
    let askpass = if let Some(password) = password {
        let directory = tempfile::Builder::new()
            .prefix("pl-ssh-askpass-")
            .tempdir()
            .map_err(|error| {
                RemoteClientError::Protocol(format!(
                    "failed to create SSH askpass directory: {error}"
                ))
            })?;
        let path = directory.path().join("askpass");
        write_askpass(&path).await?;
        command
            .arg("-o")
            .arg("NumberOfPasswordPrompts=1")
            .arg("-o")
            .arg("PubkeyAuthentication=no")
            .env("SSH_ASKPASS", &path)
            .env("SSH_ASKPASS_REQUIRE", "force")
            .env("DISPLAY", "anywork")
            .env("PURE_SSH_PASSWORD", password);
        Some(directory)
    } else {
        None
    };
    command.arg("--").arg(&profile.host);
    configure_background_command(&mut command);
    Ok(PreparedSshCommand { command, askpass })
}

async fn write_askpass(path: &std::path::Path) -> Result<(), RemoteClientError> {
    const SCRIPT: &str = "#!/bin/sh\nprintf '%s\\n' \"$PURE_SSH_PASSWORD\"\n";
    #[cfg(unix)]
    {
        // A writable fd in this multithreaded process could be inherited by an unrelated
        // concurrent fork, even with CLOEXEC. The isolated writer owns every writable fd
        // and is reaped before the script can be executed. No secret is passed to it.
        let mut writer = Command::new("/bin/sh");
        writer
            .args([
                "-c",
                "umask 077; printf '%s' \"$2\" > \"$1\" && chmod 700 \"$1\"",
                "askpass-writer",
            ])
            .arg(path)
            .arg(SCRIPT)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped());
        configure_background_command(&mut writer);
        let output = writer.output().await.map_err(|error| {
            RemoteClientError::Protocol(format!("failed to write SSH askpass: {error}"))
        })?;
        if !output.status.success() {
            return Err(RemoteClientError::Protocol(format!(
                "SSH askpass writer failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
    }
    #[cfg(not(unix))]
    tokio::fs::write(path, SCRIPT).await?;
    Ok(())
}

/// OpenSSH invokes the account shell; all bootstrap syntax belongs to POSIX sh.
pub(super) fn posix_remote_command(script: &str) -> String {
    format!("exec /bin/sh -c '{}'", script.replace('\'', "'\\''"))
}

pub(super) async fn run_ssh_capture(
    profile: &SshServerProfile,
    password: Option<&str>,
    remote_command: &str,
) -> Result<String, RemoteClientError> {
    let mut prepared = ssh_command(profile, password).await?;
    prepared.command.arg(posix_remote_command(remote_command));
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

/// Owns the child through timeout cleanup. Dropping the caller also kills the child,
/// with Tokio retaining responsibility for reaping it.
pub(super) async fn run_bounded_ssh(
    command: &mut Command,
    input: Option<&[u8]>,
    stage: &'static str,
    timeout: std::time::Duration,
) -> Result<std::process::Output, RemoteClientError> {
    use tokio::io::AsyncWriteExt;
    command
        .stdin(if input.is_some() {
            std::process::Stdio::piped()
        } else {
            std::process::Stdio::null()
        })
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(|error| {
        RemoteClientError::Protocol(format!("SSH {stage} spawn failed: {error}"))
    })?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdin = child.stdin.take();
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
            child.kill().await.map_err(|error| {
                RemoteClientError::Protocol(format!(
                    "SSH {stage} cleanup failed after {reason}: {error}"
                ))
            })?;
            Err(RemoteClientError::Protocol(format!("SSH {stage} {reason}")))
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_ssh_child_fixture() {
        if std::env::var_os("PURE_SSH_TIMEOUT_FIXTURE").is_some() {
            std::thread::park();
        }
    }

    #[tokio::test]
    async fn stalled_probe_is_reaped_and_a_following_probe_can_complete() {
        let mut stalled = Command::new(std::env::current_exe().unwrap());
        stalled
            .args([
                "--exact",
                "remote::manager::ssh::tests::bounded_ssh_child_fixture",
            ])
            .env("PURE_SSH_TIMEOUT_FIXTURE", "1");
        let result = run_bounded_ssh(
            &mut stalled,
            None,
            "probe",
            std::time::Duration::from_millis(200),
        )
        .await;
        assert!(result.unwrap_err().to_string().contains("probe timed out"));
        let mut next = Command::new(std::env::current_exe().unwrap());
        next.args([
            "--exact",
            "remote::manager::ssh::tests::bounded_ssh_child_fixture",
        ]);
        let output = run_bounded_ssh(&mut next, None, "probe", std::time::Duration::from_secs(10))
            .await
            .unwrap();
        assert!(output.status.success());
    }

    fn profile() -> SshServerProfile {
        SshServerProfile {
            id: "server-1".to_string(),
            name: "Development".to_string(),
            host: "example.test".to_string(),
            port: 2222,
            username: "dev".to_string(),
            auth: SshAuth::AgentOrKey {
                identity_file: None,
            },
        }
    }

    #[cfg(unix)]
    #[test]
    fn posix_bootstrap_preserves_quoted_arguments() {
        let script = posix_remote_command("printf '%s' \"a'b \\\"c\\\"\"");
        let output = std::process::Command::new("/bin/sh")
            .args(["-c", &script])
            .output()
            .expect("POSIX shell");
        assert!(output.status.success());
        assert_eq!(output.stdout, b"a'b \"c\"");
    }

    #[tokio::test]
    async fn command_uses_stdio_only_transport() {
        let prepared = ssh_command(&profile(), None)
            .await
            .expect("valid SSH profile");
        let args = prepared
            .command
            .as_std()
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            args,
            vec![
                "-T",
                "-x",
                "-o",
                "ConnectTimeout=15",
                "-o",
                "ServerAliveInterval=15",
                "-o",
                "ServerAliveCountMax=3",
                "-p",
                "2222",
                "-l",
                "dev",
                "--",
                "example.test"
            ]
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn password_askpass_is_executable_after_preparation() {
        let prepared = ssh_command(&profile(), Some("leased-secret"))
            .await
            .expect("SSH command");
        let askpass = prepared.askpass.expect("askpass lease");
        let output = std::process::Command::new(askpass.path().join("askpass"))
            .env("PURE_SSH_PASSWORD", "leased-secret")
            .output()
            .expect("execute askpass");

        assert!(output.status.success());
        assert_eq!(output.stdout, b"leased-secret\n");
        assert!(output.stderr.is_empty());
    }
}
