//! Product resource and write-behind owners shared by the unified Thread factory.
mod repository;
pub(in crate::studio) mod workspace_preparation;
pub(in crate::studio) mod worktree_lease;
pub(in crate::studio) use repository::ThreadWriteBehindWriter;
