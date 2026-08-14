mod connector;
mod health;
mod naming;
mod output;
mod runtime;

pub use connector::{ConnectedMcp, McpConnectRequest, McpConnector};
pub use health::{McpAvailabilityKind, McpAvailabilitySnapshot};
pub use runtime::{
    McpGeneration, McpResetScope, McpRuntime, McpRuntimeHandle, McpRuntimeToolDescriptor,
    McpTurnLease,
};

const MCP_TOOL_PREFIX: &str = "mcp__";

pub(crate) fn is_mcp_tool_name(name: &str) -> bool {
    name.starts_with(MCP_TOOL_PREFIX)
}

#[cfg(test)]
mod tests;
#[cfg(test)]
pub(crate) use tests::McpTestHarness;
