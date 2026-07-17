mod matcher;
mod parser;

pub(crate) use matcher::apply_chunks;
pub use parser::{Hunk, parse_patch};

fn tool_error(error: impl std::fmt::Display) -> pl_protocol::PureError {
    pl_protocol::PureError::ToolExecutionFailed {
        tool: "apply_patch".to_string(),
        error: error.to_string(),
    }
}
