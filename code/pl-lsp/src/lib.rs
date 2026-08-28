//! pl-lsp：数据驱动 catalog 的多语言 LSP runtime。
//!
//! catalog（[`LspServerDefinition`]）是纯数据，driver（[`LspServerDriver`]）是
//! server 生命周期的唯一 adapter 边界；registry 与路由层不含语言专项逻辑。

mod catalog;
mod client;
mod client_config;
mod client_retry;
mod client_server;
mod diagnostics;
mod driver;
mod formatting;
mod host;
mod process;
mod registry;
mod resolved;
mod rpc;
mod status;
mod time;
mod transport;
mod types;
mod uri;

pub use catalog::{
    LspCatalogError, LspCatalogServer, LspCommandSpec, LspServerCatalog, LspServerDefinition,
    LspUserServerConfig, RUST_ANALYZER_ID,
};
pub use driver::{LspProbeOutcome, LspRepairError, LspResolvedCommand, LspServerDriver};
pub use host::{
    LspHostBackend, LspHostError, LspHostFileStat, LspHostProcess, LspHostProcessExit,
    LspHostSpawnRequest,
};
pub use registry::LspRuntimeRegistry;
pub use types::*;
