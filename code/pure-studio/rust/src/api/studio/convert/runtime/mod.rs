//! Studio runtime 域模型到 FRB DTO 的桥接目录页。
//!
//! 按域拆分:`snapshot` 桥接 lifecycle 快照,`recovery` 桥接恢复问题与 Task 会话恢复,
//! `task` 桥接 Task runtime 聚合,`health` 桥接 MCP/LSP/agent 观测状态。

mod health;
mod observed;
mod recovery;
mod snapshot;
mod task;
mod updater;

pub(crate) use health::*;
pub(crate) use observed::*;
pub(crate) use recovery::*;
pub(crate) use snapshot::*;
pub(crate) use task::*;
pub(crate) use updater::*;
