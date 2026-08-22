//! StudioRuntime 的生命周期实现。
//!
//! 职责划分:
//! - `builder`: 构造与装配(进程锁、store、config 与各子 runtime);
//! - `framework`: agent framework 的启动/关闭、Thread 驻留与订阅访问;
//! - `transitions`: runtime 状态机的 initialize / start / shutdown 转换;
//! - `snapshot`: runtime 快照与设置/恢复问题状态发布;
//! - `external_state`: provider usage、update 与 LSP 状态的读操作;
//! - `project`: Project 激活与 skills / LSP 的显式操作;
//! - `recovery`: 重启后恢复问题的收集。

mod builder;
mod external_state;
mod framework;
mod project;
mod recovery;
mod snapshot;
mod transitions;
