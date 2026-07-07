use std::process::Stdio;

use pl_protocol::{PureError, Result};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use super::backend::{
    ContainerBackend, ContainerCopyFromRequest, ContainerCopyToRequest, ContainerExecOutput,
    ContainerExecRequest,
};
use super::helpers::shell_quote;

/// 基于 Docker CLI 的通用容器后端。
///
/// 该实现只封装 `docker exec` / `docker cp` 的通用能力，不管理镜像、volume、
/// label、sidecar 或凭证注入；这些产品策略由上层通过 workspace/profile 决定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockerCliContainerBackend {
    binary: String,
    container_id: String,
}

impl DockerCliContainerBackend {
    pub fn new(container_id: impl Into<String>) -> Self {
        Self {
            binary: "docker".to_string(),
            container_id: container_id.into(),
        }
    }

    pub fn with_binary(container_id: impl Into<String>, binary: impl Into<String>) -> Self {
        Self {
            binary: binary.into(),
            container_id: container_id.into(),
        }
    }

    pub fn container_id(&self) -> &str {
        &self.container_id
    }
}

impl ContainerBackend for DockerCliContainerBackend {
    async fn exec(&self, request: ContainerExecRequest) -> Result<ContainerExecOutput> {
        let shell_command = shell_command_with_optional_timeout(
            &request.command,
            request.timeout_secs.filter(|seconds| *seconds > 0),
        );
        let mut command = Command::new(&self.binary);
        command.arg("exec");
        if let Some(cwd) = &request.cwd {
            command.args(["-w", cwd]);
        }
        command.args([&self.container_id, "/bin/sh", "-lc", &shell_command]);
        let output = command.output().await.map_err(docker_error)?;
        let stdout_bytes = output.stdout.len() as u64;
        let stderr_bytes = output.stderr.len() as u64;
        let cap = request.output_bytes_cap.unwrap_or(usize::MAX);
        let (stdout, stdout_truncated) = decode_and_limit(output.stdout, cap);
        let (stderr, stderr_truncated) = decode_and_limit(output.stderr, cap);
        Ok(ContainerExecOutput {
            status: output.status.code().unwrap_or(-1),
            stdout,
            stderr,
            stdout_truncated,
            stderr_truncated,
            stdout_bytes,
            stderr_bytes,
            output_artifacts: Vec::new(),
        })
    }

    async fn copy_from(&self, request: ContainerCopyFromRequest) -> Result<Vec<u8>> {
        if request.archive {
            let source = format!("{}:{}", self.container_id, request.path);
            let output = Command::new(&self.binary)
                .args(["cp", &source, "-"])
                .output()
                .await
                .map_err(docker_error)?;
            if !output.status.success() {
                return Err(docker_error(stderr_or_stdout(&output)));
            }
            return Ok(output.stdout);
        }

        let output = Command::new(&self.binary)
            .args([
                "exec",
                &self.container_id,
                "/bin/sh",
                "-lc",
                &format!("cat -- {}", shell_quote(&request.path)),
            ])
            .output()
            .await
            .map_err(docker_error)?;
        if !output.status.success() {
            return Err(docker_error(stderr_or_stdout(&output)));
        }
        Ok(output.stdout)
    }

    async fn copy_to(&self, request: ContainerCopyToRequest) -> Result<()> {
        let parent = parent_dir(&request.path);
        let copy_command = if parent.is_empty() {
            format!("cat > {}", shell_quote(&request.path))
        } else {
            format!(
                "mkdir -p {} && cat > {}",
                shell_quote(&parent),
                shell_quote(&request.path)
            )
        };
        let mut child = Command::new(&self.binary)
            .args([
                "exec",
                "-i",
                &self.container_id,
                "/bin/sh",
                "-lc",
                &copy_command,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(docker_error)?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| docker_error("docker exec stdin pipe unavailable while copying file"))?;
        stdin
            .write_all(&request.content)
            .await
            .map_err(docker_error)?;
        drop(stdin);
        let output = child.wait_with_output().await.map_err(docker_error)?;
        if !output.status.success() {
            return Err(docker_error(stderr_or_stdout(&output)));
        }
        Ok(())
    }
}

fn shell_command_with_optional_timeout(command: &str, timeout_secs: Option<u64>) -> String {
    match timeout_secs {
        Some(seconds) => format!(
            "timeout --preserve-status {seconds}s /bin/sh -lc {}",
            shell_quote(command)
        ),
        None => command.to_string(),
    }
}

fn decode_and_limit(bytes: Vec<u8>, cap: usize) -> (String, bool) {
    let text = String::from_utf8_lossy(&bytes).to_string();
    if text.len() <= cap {
        return (text, false);
    }
    let mut end = cap;
    while !text.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    (text[..end].to_string(), true)
}

fn parent_dir(path: &str) -> String {
    path.rsplit_once('/')
        .map(|(parent, _)| {
            if parent.is_empty() {
                "/".to_string()
            } else {
                parent.to_string()
            }
        })
        .unwrap_or_default()
}

fn stderr_or_stdout(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    } else {
        stderr
    }
}

fn docker_error(error: impl std::fmt::Display) -> PureError {
    PureError::ToolExecutionFailed {
        tool: "docker".to_string(),
        error: error.to_string(),
    }
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{SystemTime, UNIX_EPOCH};

    use pretty_assertions::assert_eq;

    use super::*;

    #[tokio::test]
    async fn docker_cli_backend_exec_uses_configured_binary() {
        let (script, log) = fake_docker_script();
        let backend =
            DockerCliContainerBackend::with_binary("container-1", script.to_string_lossy());

        let output = backend
            .exec(ContainerExecRequest {
                command: "printf hello".to_string(),
                cwd: Some("/workspace".to_string()),
                timeout_secs: Some(3),
                output_bytes_cap: Some(100),
                cancellation_token: None,
            })
            .await
            .expect("exec");

        assert_eq!(output.status, 0);
        assert_eq!(output.stdout, "hello");
        let args = read_logged_args(log);
        assert_eq!(
            args,
            vec![
                "exec",
                "-w",
                "/workspace",
                "container-1",
                "/bin/sh",
                "-lc",
                "timeout --preserve-status 3s /bin/sh -lc 'printf hello'",
            ]
        );
    }

    #[tokio::test]
    async fn docker_cli_backend_copy_to_streams_stdin() {
        let (script, log) = fake_docker_script();
        let backend =
            DockerCliContainerBackend::with_binary("container-1", script.to_string_lossy());

        backend
            .copy_to(ContainerCopyToRequest {
                path: "/tmp/file.txt".to_string(),
                content: b"payload".to_vec(),
            })
            .await
            .expect("copy");

        let args = read_logged_args(log);
        assert_eq!(
            args,
            vec![
                "exec",
                "-i",
                "container-1",
                "/bin/sh",
                "-lc",
                "mkdir -p /tmp && cat > /tmp/file.txt",
            ]
        );
    }

    fn read_logged_args(path: std::path::PathBuf) -> Vec<String> {
        fs::read_to_string(path)
            .expect("log")
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn fake_docker_script() -> (std::path::PathBuf, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "pl-core-docker-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("dir");
        let script = dir.join("docker");
        let log = dir.join("args.log");
        fs::write(
            &script,
            format!(
                "#!/bin/sh\n: > {}\nfor arg in \"$@\"; do printf '%s\\n' \"$arg\" >> {}; done\nif [ \"$1\" = exec ]; then cat >/dev/null; printf hello; exit 0; fi\nif [ \"$1\" = cp ]; then printf tar; exit 0; fi\nexit 2\n",
                shell_quote(&log.to_string_lossy()),
                shell_quote(&log.to_string_lossy())
            ),
        )
        .expect("script");
        let mut permissions = fs::metadata(&script).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).expect("chmod");
        (script, log)
    }
}
