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
        command.kill_on_drop(true);
        let child = command.spawn().map_err(|error| {
            LocalExecutionFailure::BeforeSpawn(format!("failed to run command: {error}"))
        })?;
        let output = match request.timeout {
            Some(timeout) => tokio::time::timeout(timeout, child.wait_with_output())
                .await
                .map_err(|_| LocalExecutionFailure::TimedOut)?,
            None => child.wait_with_output().await,
        }
        .map_err(|error| {
            LocalExecutionFailure::AfterSpawn(format!("failed to run command: {error}"))
        })?;
        Ok(ExecutionOutput {
            status: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }
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
