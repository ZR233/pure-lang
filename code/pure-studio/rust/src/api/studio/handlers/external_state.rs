use crate::api::studio::bridge_runtime::active_bridge;
use crate::api::studio::convert::runtime::{bridge_lsp_state, bridge_mcp_state};
use crate::api::studio::types::{
    BridgeError, BridgeLspStateSnapshot, BridgeMcpStateSnapshot, LspScopeInput, McpResetInput,
};

pub async fn read_mcp_state() -> Result<BridgeMcpStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    let state = bridge.studio.read_mcp_state().await?;
    Ok(bridge_mcp_state(state.state))
}

pub async fn reset_mcp(input: McpResetInput) -> Result<BridgeMcpStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    let request = match input {
        McpResetInput::Server { server_id } => {
            pl_protocol::studio::McpResetRequest::Server { server_id }
        }
        McpResetInput::All => pl_protocol::studio::McpResetRequest::All,
    };
    bridge.studio.reset_mcp(request).await?;
    read_mcp_state().await
}

pub async fn read_lsp_state() -> Result<BridgeLspStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    let state = bridge.studio.read_lsp_state().await;
    Ok(bridge_lsp_state(state.state))
}

pub async fn probe_lsp_server(project_id: String) -> Result<BridgeLspStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    bridge.studio.probe_lsp_server(&project_id).await?;
    read_lsp_state().await
}

pub async fn repair_lsp_server(
    project_id: String,
    server_id: String,
) -> Result<BridgeLspStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    bridge
        .studio
        .repair_lsp_server(&project_id, &server_id)
        .await?;
    read_lsp_state().await
}

pub async fn reset_lsp(input: LspScopeInput) -> Result<BridgeLspStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    let request = match input {
        LspScopeInput::Server {
            project_id,
            server_id,
        } => pl_protocol::studio::LspResetRequest::Server {
            project_id,
            server_id,
        },
        LspScopeInput::Workspace { project_id } => {
            pl_protocol::studio::LspResetRequest::Workspace { project_id }
        }
        LspScopeInput::All => pl_protocol::studio::LspResetRequest::All,
    };
    bridge.studio.reset_lsp_request(request).await?;
    read_lsp_state().await
}
