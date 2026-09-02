//! LSP workspace 与 language server 进程的宿主边界。

mod backend;
mod process;

pub use backend::{
    LspHostBackend, LspHostError, LspHostFileStat, LspHostProcess, LspHostProcessExit,
    LspHostSpawnRequest,
};
pub(crate) use backend::{LspHostReader, LspHostWriter};
pub(crate) use process::{LspChild, spawn_background};
