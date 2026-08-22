//! Codex 风格 `apply_patch` 的解析与执行。
//!
//! 职责划分:`parse` 把 patch 文本解析为 hunk,`apply` 通过 `PatchBackend`
//! 执行 hunk 并产出 `PatchOutcome`,`matching` 承载上下文行匹配启发式,
//! `backend` 定义文件系统抽象,`error` 提供统一错误类型。

mod apply;
mod backend;
mod error;
mod matching;
mod outcome;
mod parse;
#[cfg(test)]
mod unit_tests;

pub use apply::*;
pub use backend::*;
pub use error::*;
pub use outcome::*;
pub use parse::*;
