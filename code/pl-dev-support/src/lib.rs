//! 工程工具（xtask 与独立验收工具）共享的仓库路径与命令执行支持。
//!
//! 本 crate 只依赖基础与平台库，不链接 Studio 运行时、模型、数据库或模拟
//! 供应商；crate 边界见 `design/02-crates.md`。

pub mod paths;
pub mod process;
