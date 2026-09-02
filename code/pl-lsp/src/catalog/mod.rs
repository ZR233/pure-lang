//! LSP server 定义、workspace 检测与 driver catalog。

mod builtin;
mod collection;
mod definition;
mod matching;

pub use builtin::RUST_ANALYZER_ID;
pub use collection::*;
pub use definition::*;

pub(crate) use matching::glob_match;
