/// Studio task agent worktree 管理错误。
///
/// 该类型只在 [`super::WorktreeManager`] 内部使用；对外统一映射为
/// `pl_protocol::PureError::ToolExecutionFailed { tool: "worktree", error }`，
/// 不跨 crate 新增错误枚举变体。
#[derive(Debug, thiserror::Error)]
pub enum WorktreeError {
    /// 无法解析 repo 根（不在 git 仓库或路径不存在）。
    #[error("invalid repo root: {0}")]
    InvalidRepoRoot(String),
    /// 分支名不安全，被 `GitPolicy::validate_branch` 拒绝。
    #[error("unsafe git branch `{0}`")]
    UnsafeBranch(String),
    /// git 命令执行失败（退出码非零或启动失败）。
    #[error("git {args} failed{}", git_stderr_suffix(.stderr))]
    GitCommand { args: String, stderr: String },
    /// 文件系统操作失败。
    #[error("worktree io error: {0}")]
    Io(String),
    /// worktree 支持未启用。
    #[error("worktree support is disabled")]
    Disabled,
    /// 一个资源操作失败，且其补偿清理也失败。
    #[error("{operation}; rollback failed: {cleanup}")]
    OperationFailedWithCleanup {
        operation: Box<WorktreeError>,
        cleanup: Box<WorktreeError>,
    },
    /// 清理流程中的一个或多个独立步骤失败。
    #[error("{context} cleanup failed{}", cleanup_failures_suffix(.failures))]
    CleanupFailed {
        context: String,
        failures: Vec<WorktreeError>,
    },
}

fn git_stderr_suffix(stderr: &str) -> String {
    let stderr = stderr.trim();
    if stderr.is_empty() {
        String::new()
    } else {
        format!(": {stderr}")
    }
}

fn cleanup_failures_suffix(failures: &[WorktreeError]) -> String {
    failures
        .iter()
        .map(|failure| format!("; {failure}"))
        .collect()
}

impl From<WorktreeError> for pl_protocol::PureError {
    fn from(error: WorktreeError) -> Self {
        Self::ToolExecutionFailed {
            tool: "worktree".to_string(),
            error: error.to_string(),
        }
    }
}
