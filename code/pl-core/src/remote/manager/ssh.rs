//! OpenSSH argv、Askpass lease 与一次性命令边界。

use std::io::Write;

use tokio::process::Command;

use super::{SshAuth, SshServerProfile};
use crate::process::configure_background_command;
use crate::remote::RemoteClientError;

pub(super) struct PreparedSshCommand {
    pub(super) command: Command,
    pub(super) askpass: Option<tempfile::TempPath>,
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

pub(super) fn ssh_command(
    profile: &SshServerProfile,
    password: Option<&str>,
) -> Result<PreparedSshCommand, RemoteClientError> {
    let mut command = Command::new("ssh");
    command
        .arg("-T")
        // SSH 进程只承载 stdio 协议，不需要 X11；显式关闭可避免用户 ssh
        // 配置中的 ForwardX11 设置向远端注入图形会话并产生 xauth 警告。
        .arg("-x")
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
        let mut file = tempfile::Builder::new()
            .prefix("pl-ssh-askpass-")
            .tempfile()
            .map_err(|error| {
                RemoteClientError::Protocol(format!("failed to create SSH askpass: {error}"))
            })?;
        file.write_all(b"#!/bin/sh\nprintf '%s\\n' \"$PURE_SSH_PASSWORD\"\n")
            .and_then(|()| file.flush())
            .map_err(|error| {
                RemoteClientError::Protocol(format!("failed to write SSH askpass: {error}"))
            })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(file.path(), std::fs::Permissions::from_mode(0o700)).map_err(
                |error| {
                    RemoteClientError::Protocol(format!("failed to secure SSH askpass: {error}"))
                },
            )?;
        }
        let path = file.into_temp_path();
        command
            .arg("-o")
            .arg("NumberOfPasswordPrompts=1")
            .arg("-o")
            .arg("PubkeyAuthentication=no")
            .env("SSH_ASKPASS", &path)
            .env("SSH_ASKPASS_REQUIRE", "force")
            .env("DISPLAY", "pure-studio")
            .env("PURE_SSH_PASSWORD", password);
        Some(path)
    } else {
        None
    };
    command.arg("--").arg(&profile.host);
    configure_background_command(&mut command);
    Ok(PreparedSshCommand { command, askpass })
}

pub(super) async fn run_ssh_capture(
    profile: &SshServerProfile,
    password: Option<&str>,
    remote_command: &str,
) -> Result<String, RemoteClientError> {
    let mut prepared = ssh_command(profile, password)?;
    let output = prepared
        .command
        .arg(remote_command)
        .output()
        .await
        .map_err(|error| RemoteClientError::Protocol(format!("failed to start ssh: {error}")))?;
    if !output.status.success() {
        return Err(RemoteClientError::Protocol(format!(
            "ssh command failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    String::from_utf8(output.stdout)
        .map_err(|error| RemoteClientError::Protocol(format!("ssh output is not UTF-8: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn command_uses_stdio_only_transport() {
        let prepared = ssh_command(&profile(), None).expect("valid SSH profile");
        let args = prepared
            .command
            .as_std()
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            args,
            vec!["-T", "-x", "-p", "2222", "-l", "dev", "--", "example.test"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn password_askpass_is_executable_after_preparation() {
        let prepared = ssh_command(&profile(), Some("leased-secret")).expect("SSH command");
        let askpass = prepared.askpass.expect("askpass lease");
        let output = std::process::Command::new(&askpass)
            .env("PURE_SSH_PASSWORD", "leased-secret")
            .output()
            .expect("execute askpass");

        assert!(output.status.success());
        assert_eq!(output.stdout, b"leased-secret\n");
        assert!(output.stderr.is_empty());
    }
}
