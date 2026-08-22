//! 工具编排:一次 Turn 冻结的工具目录与 client tool search。
//!
//! 按域拆分:`inventory` 承载 eager/延迟分配与 schema 投影,`search` 承载
//! client `tool_search` 的 catalog 检索、调用解析与 wire 投影。

mod inventory;
mod search;

#[cfg(test)]
mod unit_tests;

pub use inventory::*;
pub(crate) use search::{ClientToolSearchCallSummary, ClientToolSearchResolution};
