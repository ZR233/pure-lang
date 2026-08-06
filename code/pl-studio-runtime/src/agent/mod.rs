//! Pure Studio agent 外部资源基础设施。
//!
//! agent/session/turn 状态机位于 `agent_runtime`；本模块只保留可由 host lifecycle
//! 复用的 worktree 原语。

pub mod worktree;

pub use worktree::{
    DurableWorktreeDisposition, DurableWorktreePresence, DurableWorktreeResource,
    LocalWorktreeBackend, WorktreeBackend, WorktreeCreateFailure, WorktreeCreateSpec,
    WorktreeError, WorktreeHandle, WorktreeManager, WorktreeReconciliation, WorktreeRef,
    reconcile_task_worktree_group, same_worktree_path,
};
