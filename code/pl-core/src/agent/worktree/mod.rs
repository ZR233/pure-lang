//! Subagent worktree 隔离执行支持。
//!
//! 为每个 subagent 分配独立 git worktree，使其文件修改物理隔离；subagent 关闭时
//! 由调用方选择把产物 `merge` 回主工作区或 `discard` 丢弃，worktree 随 subagent
//! 释放。详见 `design/15-agent-worktrees.md`。

mod backend;
mod error;
mod manager;
#[cfg(test)]
mod tests;

pub use backend::{LocalWorktreeBackend, MergeOutcome, WorktreeBackend, WorktreeCreateFailure};
pub use error::WorktreeError;
pub(crate) use manager::git_compatible_path;
pub use manager::{
    CloseDisposition, CloseOutcome, WorktreeCreateSpec, WorktreeHandle, WorktreeManager,
    WorktreeRef,
};
