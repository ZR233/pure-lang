mod client;
mod contract;
mod health;
mod local_host;
mod naming;
mod runtime;
mod tool_adapter;
mod transport;
mod wire;

pub use contract::{
    McpCallRequest, McpConnectRequest, McpRuntimeHost, McpSession, McpToolDefinition,
};
pub use health::{McpAvailabilityKind, McpAvailabilitySnapshot};
pub use local_host::{LocalMcpRuntimeHost, LocalMcpSession};
pub use runtime::{
    McpGeneration, McpRuntime, McpRuntimeHandle, McpRuntimeToolDescriptor, McpTurnLease,
};

const MCP_TOOL_PREFIX: &str = "mcp__";

pub(crate) fn is_mcp_tool_name(name: &str) -> bool {
    name.starts_with(MCP_TOOL_PREFIX)
}

#[cfg(test)]
mod tests;
