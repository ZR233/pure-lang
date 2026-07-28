//! Studio task agent worktree 隔离执行支持。
//!
//! 为每个 subagent 分配独立 git worktree，使其文件修改物理隔离；subagent 关闭时
//! 由调用方选择把产物 `merge` 回主工作区或 `discard` 丢弃，worktree 随 subagent
//! 释放。详见 `design/15-agent-worktrees.md`。

mod backend;
mod error;
mod manager;
mod reconcile;
#[cfg(test)]
mod tests;

pub use backend::{LocalWorktreeBackend, MergeOutcome, WorktreeBackend, WorktreeCreateFailure};
pub use error::WorktreeError;
pub use manager::git_compatible_path;
pub use manager::{
    CloseDisposition, CloseOutcome, WorktreeCreateSpec, WorktreeHandle, WorktreeManager,
    WorktreeRef,
};
pub use reconcile::{
    DurableWorktreeDisposition, DurableWorktreeInspection, DurableWorktreePresence,
    DurableWorktreeResource, DurableWorktreeResourcePresence, WorktreeReconciliation,
    cleanup_task_worktree_resources, inspect_task_worktree_resources,
    reconcile_task_worktree_group, validate_task_worktree_resource_identities,
};

/// Compares worktree paths using filesystem identity where available and
/// platform path semantics otherwise.
pub fn same_worktree_path(
    left: impl AsRef<std::path::Path>,
    right: impl AsRef<std::path::Path>,
) -> bool {
    fn key(path: &std::path::Path) -> String {
        let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let value = path.to_string_lossy().replace('\\', "/");
        if cfg!(windows) {
            value.to_lowercase()
        } else {
            value
        }
    }

    key(left.as_ref()) == key(right.as_ref())
}
#[cfg(test)]
pub(crate) use reconcile::{reconcile_task_worktrees, set_after_registration_remove_barrier};
