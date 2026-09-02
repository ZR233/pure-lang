//! LSP server 环境探测、repair 与初始化的 adapter 边界。

mod api;
pub(crate) mod command;
mod probe;
pub(crate) mod rust_analyzer;

pub use api::*;

pub(crate) use probe::{CommandProbeError, PROBE_TIMEOUT, run_command_capture};
