//! Image snapshots and decoding limits supplied by the host, independent of model protocols.

mod normalize;
mod thread;
mod view;
pub use thread::ThreadViewImageTool;

pub(crate) use normalize::*;
pub use view::{TOOL_VIEW_IMAGE, ViewImageInput};

/// Host-selected limits for image output. The host first verifies model source/replay support.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageOutputLimits {
    pub max_count: Option<u32>,
    pub max_bytes: Option<u64>,
    pub max_total_bytes: Option<u64>,
    pub max_width: Option<u32>,
    pub max_height: Option<u32>,
    pub media_types: Vec<String>,
}
