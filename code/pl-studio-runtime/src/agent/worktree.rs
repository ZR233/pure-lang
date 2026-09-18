//! Studio child Agent 的 Git worktree 隔离实现。

use std::path::Path;

mod backend;
mod manager;
mod remote;

pub use backend::{
    LocalWorktreeBackend, WorktreeBackend, WorktreeCreateFailure, WorktreeCreateFailureDisposition,
    WorktreeError, WorktreeStatus,
};
pub use manager::{
    WorktreeCreateSpec, WorktreeHandle, WorktreeManager, WorktreeOwnership, is_pure_branch,
};
pub use remote::RemoteWorktreeBackend;

/// 远端跨端路径的唯一规范化表达。
///
/// 任何宿主形态的路径（含客户端 Windows 路径分隔符与驱动器前缀风格的本地表示）在跨端前
/// 都必须归一化为 POSIX 字符串。远端 git 路径参数、目录/文件操作、workspace 打开参数与
/// durable lease 记录共用该结果，从而保证物理 worktree 与 lease 记录落在同一位置。
/// 本地项目路径保持宿主形态，不经过该函数。
pub(crate) fn remote_path_text(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
