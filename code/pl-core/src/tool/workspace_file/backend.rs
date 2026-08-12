use std::fmt;
use std::future::Future;

use pl_protocol::Result;

/// workspace 文件工具的统一后端。
///
/// 工具层负责解析统一输入、生成统一 JSON 输出并执行 patch 匹配逻辑；
/// backend 只表达“在某个 workspace 中读、写、删、列”的能力。本地目录、
/// Docker 容器或远程沙箱都应通过实现该 trait 接入，避免为同名 file 工具维护多套协议。
pub trait WorkspaceFileBackend: fmt::Debug + Send + Sync {
    fn default_cwd(&self) -> impl Future<Output = Result<String>> + Send;

    fn stat(
        &self,
        request: WorkspaceFileStatRequest,
    ) -> impl Future<Output = Result<WorkspaceFileStat>> + Send;

    fn read_text(
        &self,
        request: WorkspaceFileReadRequest,
    ) -> impl Future<Output = Result<String>> + Send;

    fn write_text(
        &self,
        request: WorkspaceFileWriteRequest,
    ) -> impl Future<Output = Result<()>> + Send;

    fn remove_file(
        &self,
        request: WorkspaceFileRemoveRequest,
    ) -> impl Future<Output = Result<()>> + Send;

    /// Lists matching descendants without returning the requested directory itself.
    fn list(
        &self,
        request: WorkspaceFileListRequest,
    ) -> impl Future<Output = Result<WorkspaceFileListResult>> + Send;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceFileStatRequest {
    pub path: String,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceFileStat {
    pub path: String,
    pub is_file: bool,
    pub is_dir: bool,
    pub len: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceFileReadRequest {
    pub path: String,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceFileWriteRequest {
    pub path: String,
    pub cwd: Option<String>,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceFileRemoveRequest {
    pub path: String,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceFileListRequest {
    pub path: String,
    pub cwd: Option<String>,
    pub glob: String,
    pub max_files: usize,
    pub include_dirs: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceFileListResult {
    pub files: Vec<String>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceFileListEntry {
    pub path: String,
    pub is_dir: bool,
}
