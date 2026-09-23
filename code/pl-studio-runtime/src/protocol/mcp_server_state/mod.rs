//! MCP server 可用性状态；variant payload 只包含该状态合法的字段。

mod available;
mod checking;
mod disabled;
mod missing_credential;
mod unavailable;

pub use available::McpAvailable;
pub use checking::McpChecking;
pub use disabled::McpDisabled;
pub use missing_credential::McpMissingCredential;
pub use unavailable::McpUnavailable;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum StudioMcpServerState {
    Disabled(McpDisabled),
    MissingCredential(McpMissingCredential),
    Checking(McpChecking),
    Available(McpAvailable),
    Unavailable(McpUnavailable),
}
