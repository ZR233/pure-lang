//! LSP registry 所有的状态、错误、能力与生命周期编排。

mod capabilities;
mod capability;
mod error;
mod formatting;
mod membership;
mod probe;
mod query;
mod registry;
mod request;
mod reset;
mod routing;
mod server;
mod snapshot;
mod state;

pub use capability::*;
pub use error::*;
pub use registry::LspRuntimeRegistry;
pub use state::*;

use registry::{
    LspRuntimeServerState, LspRuntimeState, LspWorkspaceState, canonical_workspace_root,
};
pub(crate) use server::ResolvedLspServer;
