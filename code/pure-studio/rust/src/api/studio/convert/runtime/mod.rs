//! Studio runtime 域模型到 FRB DTO 的桥接目录页。
//!
//! 按域拆分：`snapshot` 桥接 lifecycle 快照，`health` 桥接 MCP/LSP/agent 观测状态。

mod health;
mod model_performance;
mod observed;
mod persistence;
mod recovery;
mod snapshot;
mod updater;

pub(crate) use health::*;
pub(crate) use model_performance::*;
pub(crate) use observed::*;
pub(crate) use persistence::*;
pub(crate) use recovery::*;
pub(crate) use snapshot::*;
pub(crate) use updater::*;
