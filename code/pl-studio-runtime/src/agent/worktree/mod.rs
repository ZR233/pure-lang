//! Studio child Agent 的 Git worktree 隔离实现。

mod backend;
mod manager;
mod remote;

pub use backend::{
    LocalWorktreeBackend, WorktreeBackend, WorktreeCreateFailure, WorktreeCreateFailureDisposition,
    WorktreeError, WorktreeStatus,
};
pub use manager::{WorktreeCreateSpec, WorktreeHandle, WorktreeManager};
pub use remote::RemoteWorktreeBackend;
