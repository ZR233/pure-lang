use pl_protocol::Result;

use crate::config::validate_mcp_identifier;

mod client;
mod registry;
mod tool_adapter;
mod transport;
mod wire;

pub use registry::{McpAvailabilityKind, McpAvailabilitySnapshot, McpRuntimeRegistry};

const MCP_TOOL_PREFIX: &str = "mcp__";

pub(crate) fn is_mcp_tool_name(name: &str) -> bool {
    name.starts_with(MCP_TOOL_PREFIX)
}

fn exposed_tool_name(server_id: &str, tool_name: &str) -> Result<String> {
    validate_mcp_identifier(server_id, "MCP server id")?;
    validate_mcp_identifier(tool_name, "MCP tool name")?;
    Ok(format!("{MCP_TOOL_PREFIX}{server_id}__{tool_name}"))
}

#[cfg(test)]
mod tests;
