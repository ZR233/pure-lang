use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;

/// 通用命令执行请求。
#[derive(Clone, PartialEq, Eq)]
pub struct ExecutionRequest {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
    pub timeout: Option<Duration>,
}

impl fmt::Debug for ExecutionRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let env = self
            .env
            .keys()
            .map(|key| {
                let value = if key.contains("TOKEN") || key.contains("PASSWORD") {
                    "[redacted]"
                } else {
                    self.env.get(key).map(String::as_str).unwrap_or_default()
                };
                (key, value)
            })
            .collect::<BTreeMap<_, _>>();
        f.debug_struct("ExecutionRequest")
            .field("program", &self.program)
            .field("args", &self.args)
            .field("cwd", &self.cwd)
            .field("env", &env)
            .field("timeout", &self.timeout)
            .finish()
    }
}

/// 通用命令执行结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

/// shell/process 类工具共用的执行后端。
///
/// 实现方负责在指定工作目录运行命令，并遵守请求中给出的环境和超时。
pub trait ExecutionBackend: fmt::Debug + Send + Sync {
    type Error: fmt::Display + Send + 'static;

    fn run(
        &self,
        request: ExecutionRequest,
    ) -> impl Future<Output = std::result::Result<ExecutionOutput, Self::Error>> + Send;
}

/// 本地进程执行后端。
#[derive(Debug, Clone, Default)]
pub struct LocalExecutionBackend;

/// 本地命令失败发生在进程启动前、启动后还是等待超时。
///
/// 需要根据副作用边界实施补偿的产品 backend 可使用该分类；普通调用方应继续
/// 通过 [`ExecutionBackend::run`] 获取稳定的字符串错误。
#[derive(Debug)]
pub enum LocalExecutionFailure {
    BeforeSpawn(String),
    AfterSpawn(String),
    TimedOut,
}

impl fmt::Display for LocalExecutionFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BeforeSpawn(error) | Self::AfterSpawn(error) => formatter.write_str(error),
            Self::TimedOut => formatter.write_str("command timed out"),
        }
    }
}

impl LocalExecutionBackend {
    /// 运行本地进程，并保留失败发生在 spawn 前后的分类信息。
    pub async fn run_classified(
        &self,
        request: ExecutionRequest,
    ) -> std::result::Result<ExecutionOutput, LocalExecutionFailure> {
        let mut command = Command::new(&request.program);
        command.args(&request.args);
        command.current_dir(&request.cwd);
        command.envs(&request.env);
        command.stdin(Stdio::null());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        let mut command = pl_remote_helper::process::wrap_background_command(command);
        let child = command.spawn().map_err(|error| {
            LocalExecutionFailure::BeforeSpawn(format!("failed to run command: {error}"))
        })?;
        let output = collect_child(child, request.timeout).await?;
        Ok(ExecutionOutput {
            status: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }
}

async fn collect_child(
    mut child: Box<dyn process_wrap::tokio::ChildWrapper>,
    timeout: Option<Duration>,
) -> Result<std::process::Output, LocalExecutionFailure> {
    let stdout = child.stdout().take();
    let stderr = child.stderr().take();
    let output = async move { tokio::join!(read_pipe(stdout), read_pipe(stderr)) };
    tokio::pin!(output);
    let timer = async move {
        match timeout {
            Some(timeout) => tokio::time::sleep(timeout).await,
            None => std::future::pending().await,
        }
    };
    tokio::pin!(timer);
    let mut status = None;
    let mut streams = None;
    let mut timed_out = false;
    let mut termination_error = None;
    while status.is_none() || streams.is_none() {
        tokio::select! {
            result = child.wait(), if status.is_none() => {
                if result.is_err() { termination_error = child.start_kill().err(); }
                status = Some(result);
            }
            result = &mut output, if streams.is_none() => streams = Some(result),
            _ = &mut timer, if !timed_out => {
                timed_out = true;
                termination_error = child.start_kill().err();
            }
        }
    }
    let Some(status) = status else {
        return Err(LocalExecutionFailure::AfterSpawn(
            "missing process exit".into(),
        ));
    };
    let Some((stdout, stderr)) = streams else {
        return Err(LocalExecutionFailure::AfterSpawn(
            "missing output drain".into(),
        ));
    };
    let status = status.map_err(|error| {
        LocalExecutionFailure::AfterSpawn(format!("process exit wait failed: {error}"))
    })?;
    let stdout = stdout.map_err(|error| {
        LocalExecutionFailure::AfterSpawn(format!("stdout drain failed: {error}"))
    })?;
    let stderr = stderr.map_err(|error| {
        LocalExecutionFailure::AfterSpawn(format!("stderr drain failed: {error}"))
    })?;
    if let Some(error) = termination_error {
        return Err(LocalExecutionFailure::AfterSpawn(format!(
            "termination request failed before final process settlement: {error}"
        )));
    }
    if timed_out {
        return Err(LocalExecutionFailure::TimedOut);
    }
    Ok(std::process::Output {
        status,
        stdout,
        stderr,
    })
}

async fn read_pipe<R: tokio::io::AsyncRead + Unpin>(pipe: Option<R>) -> std::io::Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    let mut content = Vec::new();
    if let Some(mut pipe) = pipe {
        pipe.read_to_end(&mut content).await?;
    }
    Ok(content)
}

impl ExecutionBackend for LocalExecutionBackend {
    type Error = String;

    async fn run(
        &self,
        request: ExecutionRequest,
    ) -> std::result::Result<ExecutionOutput, Self::Error> {
        self.run_classified(request)
            .await
            .map_err(|error| error.to_string())
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn timeout_settles_descendants_before_returning() {
        let directory = tempfile::tempdir().unwrap();
        let request = ExecutionRequest {
            program: "/bin/sh".into(),
            args: vec![
                "-c".into(),
                "sleep 30 & printf '%s' \"$!\" > child.pid; wait".into(),
            ],
            cwd: directory.path().to_owned(),
            env: BTreeMap::new(),
            timeout: Some(Duration::from_secs(1)),
        };
        let result = LocalExecutionBackend.run_classified(request).await;
        assert!(matches!(result, Err(LocalExecutionFailure::TimedOut)));
        let pid = tokio::fs::read_to_string(directory.path().join("child.pid"))
            .await
            .unwrap();
        let status = tokio::fs::read_to_string(format!("/proc/{pid}/status")).await;
        assert!(
            descendant_has_exited(status).expect("failed to inspect descendant"),
            "descendant must not be running after timeout"
        );
    }

    fn descendant_has_exited(status: std::io::Result<String>) -> std::io::Result<bool> {
        match status {
            // Linux procfs can open the entry before task removal, then return ESRCH
            // (Linux errno 3) while reading it. Both cases mean the task is gone.
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound
                    || error.raw_os_error() == Some(3) =>
            {
                Ok(true)
            }
            Ok(status) => Ok(status.lines().any(|line| {
                line.starts_with("State:")
                    && (line.contains("Z (zombie)") || line.contains("X (dead)"))
            })),
            Err(error) => Err(error),
        }
    }

    #[test]
    fn descendant_exit_probe_accepts_only_terminal_states_and_task_disappearance() {
        for status in ["State:\tZ (zombie)", "State:\tX (dead)"] {
            assert!(descendant_has_exited(Ok(status.into())).unwrap());
        }
        for status in [
            "State:\tR (running)",
            "State:\tS (sleeping)",
            "invalid status",
        ] {
            assert!(!descendant_has_exited(Ok(status.into())).unwrap());
        }
        for errno in [2, 3] {
            assert!(descendant_has_exited(Err(std::io::Error::from_raw_os_error(errno))).unwrap());
        }
        for errno in [5, 13] {
            assert_eq!(
                descendant_has_exited(Err(std::io::Error::from_raw_os_error(errno)))
                    .unwrap_err()
                    .raw_os_error(),
                Some(errno),
            );
        }
    }

    #[tokio::test]
    async fn command_waits_for_both_output_streams() {
        let directory = tempfile::tempdir().unwrap();
        let output = LocalExecutionBackend
            .run_classified(ExecutionRequest {
                program: "/bin/sh".into(),
                args: vec!["-c".into(), "printf stdout; printf stderr >&2".into()],
                cwd: directory.path().to_owned(),
                env: BTreeMap::new(),
                timeout: Some(Duration::from_secs(5)),
            })
            .await
            .unwrap();
        pretty_assertions::assert_eq!(
            output,
            ExecutionOutput {
                status: 0,
                stdout: "stdout".into(),
                stderr: "stderr".into()
            }
        );
    }
}
