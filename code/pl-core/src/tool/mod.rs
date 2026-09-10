//! Framework tool declarations, trusted execution controls and immutable producer output.
pub mod execution_policy;
pub mod opaque;
mod output;
pub use output::{ToolControl, ToolOutput};
