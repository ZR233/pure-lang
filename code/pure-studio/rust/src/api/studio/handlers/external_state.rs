use crate::api::studio::bridge_runtime::active_bridge;
use crate::api::studio::convert::runtime::bridge_mcp_health;
use crate::api::studio::types::{
    BridgeError, BridgeLspStateSnapshot, BridgeMcpStateSnapshot, LspScopeInput, McpResetInput,
};

pub async fn read_mcp_state() -> Result<BridgeMcpStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    let state = bridge.studio.read_mcp_state().await?;
    Ok(BridgeMcpStateSnapshot {
        meta: state.meta.into(),
        desired_config_fingerprint: state.desired_config_fingerprint,
        applied_config_fingerprint: state.applied_config_fingerprint,
        health: bridge_mcp_health(state.health),
    })
}

pub async fn reset_mcp(input: McpResetInput) -> Result<BridgeMcpStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    let scope = match input {
        McpResetInput::Server { server_id } => {
            pl_studio_runtime::McpResetScope::Server { server_id }
        }
        McpResetInput::All => pl_studio_runtime::McpResetScope::All,
    };
    bridge.studio.reset_mcp(scope).await?;
    read_mcp_state().await
}

pub async fn read_lsp_state() -> Result<BridgeLspStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    let state = bridge.studio.read_lsp_state().await;
    Ok(BridgeLspStateSnapshot {
        meta: state.meta.into(),
        health: state.health.into(),
    })
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
    let scope = match input {
        LspScopeInput::Server {
            project_id,
            server_id,
        } => pl_studio_runtime::LspScope::Server {
            workspace_root: bridge.studio.project_workspace_root(&project_id).await?,
            server_id,
        },
        LspScopeInput::Workspace { project_id } => pl_studio_runtime::LspScope::Workspace {
            workspace_root: bridge.studio.project_workspace_root(&project_id).await?,
        },
        LspScopeInput::All => pl_studio_runtime::LspScope::All,
    };
    bridge.studio.reset_lsp(scope).await?;
    read_lsp_state().await
}
