//! LSP 文件与进程宿主边界。
//!
//! LSP JSON-RPC、路由、诊断与 server driver 始终留在本地；宿主只提供读取
//! workspace 文件以及启动、观察、终止 language server 的原语。

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use futures::future::BoxFuture;
use tokio::io::{AsyncRead, AsyncWrite};

/// 宿主文件元数据，只暴露 LSP 本地逻辑需要的事实。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LspHostFileStat {
    pub is_file: bool,
    pub byte_size: u64,
}

/// 启动 language server 所需的传输中立参数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspHostSpawnRequest {
    pub process_id: String,
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
}

/// 宿主进程的最终退出状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LspHostProcessExit {
    pub exit_code: Option<i32>,
}

/// LSP 宿主原语失败。
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct LspHostError {
    message: String,
}

impl LspHostError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

pub(crate) type LspHostReader = Pin<Box<dyn AsyncRead + Send>>;
pub(crate) type LspHostWriter = Pin<Box<dyn AsyncWrite + Send>>;
type LspHostWait =
    Pin<Box<dyn Future<Output = Result<LspHostProcessExit, LspHostError>> + Send + 'static>>;
type LspHostTerminate = Box<dyn FnOnce() -> BoxFuture<'static, ()> + Send>;

/// 一个由宿主维护、由本地 LSP client 消费 stdio 的可观察进程。
pub struct LspHostProcess {
    stdin: Option<LspHostWriter>,
    stdout: Option<LspHostReader>,
    stderr: Option<LspHostReader>,
    wait: Option<LspHostWait>,
    terminate: Option<LspHostTerminate>,
}

impl std::fmt::Debug for LspHostProcess {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LspHostProcess")
            .field("stdin", &self.stdin.is_some())
            .field("stdout", &self.stdout.is_some())
            .field("stderr", &self.stderr.is_some())
            .finish_non_exhaustive()
    }
}

impl LspHostProcess {
    /// 从宿主提供的 stdio、退出 future 和终止回调构造进程句柄。
    pub fn new(
        stdin: Option<LspHostWriter>,
        stdout: Option<LspHostReader>,
        stderr: Option<LspHostReader>,
        wait: impl Future<Output = Result<LspHostProcessExit, LspHostError>> + Send + 'static,
        terminate: impl FnOnce() -> BoxFuture<'static, ()> + Send + 'static,
    ) -> Self {
        Self {
            stdin,
            stdout,
            stderr,
            wait: Some(Box::pin(wait)),
            terminate: Some(Box::new(terminate)),
        }
    }

    pub(crate) fn take_stdin(&mut self) -> Option<LspHostWriter> {
        self.stdin.take()
    }

    pub(crate) fn take_stdout(&mut self) -> Option<LspHostReader> {
        self.stdout.take()
    }

    pub(crate) fn take_stderr(&mut self) -> Option<LspHostReader> {
        self.stderr.take()
    }

    pub(crate) async fn wait(&mut self) -> Result<LspHostProcessExit, LspHostError> {
        self.wait
            .take()
            .ok_or_else(|| LspHostError::new("LSP host process was already awaited"))?
            .await
    }

    pub(crate) async fn terminate(&mut self) {
        if let Some(terminate) = self.terminate.take() {
            terminate().await;
        }
    }
}

/// LSP 对 workspace 文件和 language server 进程的最小宿主依赖。
///
/// 实现不得承担 JSON-RPC、server schema、诊断、工具输出或持久化逻辑。
pub trait LspHostBackend: std::fmt::Debug + Send + Sync {
    /// 标识当前 workspace handle；transport 重连后必须变化。
    fn identity(&self) -> String;

    /// 读取一个 workspace 内文件的原始字节。
    fn read_file<'a>(
        &'a self,
        path: &'a Path,
        max_bytes: u64,
    ) -> BoxFuture<'a, Result<Vec<u8>, LspHostError>>;

    /// 查询一个 workspace 内路径，不存在返回 `None`。
    fn stat<'a>(
        &'a self,
        path: &'a Path,
    ) -> BoxFuture<'a, Result<Option<LspHostFileStat>, LspHostError>>;

    /// 列出 workspace 内目录第一层的文件名。
    fn list_directory<'a>(
        &'a self,
        path: &'a Path,
    ) -> BoxFuture<'a, Result<Vec<String>, LspHostError>>;

    /// 启动一个 stdio language server；进程树生命周期由宿主维护。
    fn spawn<'a>(
        &'a self,
        request: LspHostSpawnRequest,
    ) -> BoxFuture<'a, Result<LspHostProcess, LspHostError>>;
}
