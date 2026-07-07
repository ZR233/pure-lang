use std::fmt;
use std::future::Future;

use pl_protocol::Result;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use super::helpers::tool_error;

/// 容器内 shell 执行请求。
///
/// backend 负责把该请求映射到具体容器、沙箱或远程 workspace。调用方不得把
/// token、宿主路径等产品私有信息放入工具参数。
#[derive(Clone)]
pub struct ContainerExecRequest {
    pub command: String,
    pub cwd: Option<String>,
    pub timeout_secs: Option<u64>,
    pub output_bytes_cap: Option<usize>,
    pub cancellation_token: Option<CancellationToken>,
}

impl fmt::Debug for ContainerExecRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContainerExecRequest")
            .field("command", &self.command)
            .field("cwd", &self.cwd)
            .field("timeout_secs", &self.timeout_secs)
            .field("output_bytes_cap", &self.output_bytes_cap)
            .field(
                "cancellation_token",
                &self.cancellation_token.as_ref().map(|_| "<token>"),
            )
            .finish()
    }
}

/// 容器内 shell 执行结果。
///
/// `stdout` / `stderr` 可由 backend 预先截断；`*_bytes` 表示原始流大小，
/// `output_artifacts` 用于宿主把完整输出文件投影给 UI。
#[derive(Debug, Clone, PartialEq)]
pub struct ContainerExecOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub stdout_bytes: u64,
    pub stderr_bytes: u64,
    pub output_artifacts: Vec<Value>,
}

/// 从容器复制文件或目录的请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerCopyFromRequest {
    pub path: String,
    pub archive: bool,
}

/// 向容器写入文件的请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerCopyToRequest {
    pub path: String,
    pub content: Vec<u8>,
}

/// 容器 workspace 后端。
///
/// `pl-core` 负责解析工具参数、执行通用 file/container 语义和生成模型可见输出；
/// 产品层只实现该 trait，把请求投递到 Docker、远程沙箱或其它 workspace。
pub trait ContainerBackend: fmt::Debug + Send + Sync {
    fn exec(
        &self,
        request: ContainerExecRequest,
    ) -> impl Future<Output = Result<ContainerExecOutput>> + Send;

    fn copy_from(
        &self,
        request: ContainerCopyFromRequest,
    ) -> impl Future<Output = Result<Vec<u8>>> + Send;

    fn copy_to(&self, request: ContainerCopyToRequest) -> impl Future<Output = Result<()>> + Send;
}

/// 空容器后端，仅作为 `ToolSetBuilder` 的默认类型占位。
#[derive(Debug, Clone, Default)]
pub struct NoContainerBackend;

impl ContainerBackend for NoContainerBackend {
    async fn exec(&self, _request: ContainerExecRequest) -> Result<ContainerExecOutput> {
        Err(tool_error(
            "container",
            "container backend is not configured",
        ))
    }

    async fn copy_from(&self, _request: ContainerCopyFromRequest) -> Result<Vec<u8>> {
        Err(tool_error(
            "container",
            "container backend is not configured",
        ))
    }

    async fn copy_to(&self, _request: ContainerCopyToRequest) -> Result<()> {
        Err(tool_error(
            "container",
            "container backend is not configured",
        ))
    }
}
