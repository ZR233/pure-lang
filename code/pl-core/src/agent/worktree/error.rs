use std::fmt;

/// subagent worktree 管理错误。
///
/// 该类型只在 [`super::WorktreeManager`] 内部使用；对外统一映射为
/// `pl_protocol::PureError::ToolExecutionFailed { tool: "worktree", error }`，
/// 不跨 crate 新增错误枚举变体。
#[derive(Debug)]
pub enum WorktreeError {
    /// 无法解析 repo 根（不在 git 仓库或路径不存在）。
    InvalidRepoRoot(String),
    /// 分支名不安全，被 `GitPolicy::validate_branch` 拒绝。
    UnsafeBranch(String),
    /// git 命令执行失败（退出码非零或启动失败）。
    GitCommand { args: String, stderr: String },
    /// 文件系统操作失败。
    Io(String),
    /// worktree 支持未启用。
    Disabled,
    /// merge 冲突，worktree 未释放。
    MergeConflict { branch: String, detail: String },
}

impl fmt::Display for WorktreeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRepoRoot(msg) => write!(f, "invalid repo root: {msg}"),
            Self::UnsafeBranch(branch) => write!(f, "unsafe git branch `{branch}`"),
            Self::GitCommand { args, stderr } => {
                let stderr = stderr.trim();
                if stderr.is_empty() {
                    write!(f, "git {args} failed")
                } else {
                    write!(f, "git {args} failed: {stderr}")
                }
            }
            Self::Io(msg) => write!(f, "worktree io error: {msg}"),
            Self::Disabled => write!(f, "worktree support is disabled"),
            Self::MergeConflict { branch, detail } => {
                let detail = detail.trim();
                if detail.is_empty() {
                    write!(f, "merge conflict on branch `{branch}`")
                } else {
                    write!(f, "merge conflict on branch `{branch}`: {detail}")
                }
            }
        }
    }
}

impl std::error::Error for WorktreeError {}

impl From<WorktreeError> for pl_protocol::PureError {
    fn from(error: WorktreeError) -> Self {
        Self::ToolExecutionFailed {
            tool: "worktree".to_string(),
            error: error.to_string(),
        }
    }
}
