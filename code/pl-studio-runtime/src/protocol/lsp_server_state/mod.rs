//! LSP server 可用性与 Available 内活动状态。

mod available;
mod checking;
mod disabled;
mod unavailable;

pub use available::{LspAvailable, LspAvailableActivity, LspBusy, LspIdle, LspIndexing};
pub use checking::LspChecking;
pub use disabled::LspDisabled;
pub use unavailable::LspUnavailable;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum StudioLspServerState {
    Checking(LspChecking),
    Available(LspAvailable),
    Unavailable(LspUnavailable),
    Disabled(LspDisabled),
}
