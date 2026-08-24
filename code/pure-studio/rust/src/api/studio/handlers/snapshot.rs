use anyhow::Result;

use crate::api::studio::bridge_runtime::BridgeRuntime;
use crate::api::studio::convert::runtime::{
    bridge_agent_directory, bridge_lsp_state, bridge_mcp_state, bridge_persistence_state,
    bridge_project_directory, bridge_provider_usage_state, bridge_recovery_state,
    bridge_settings_state, bridge_skills_state, bridge_task_directory,
    bridge_thread_directory_page, bridge_update_state, runtime_snapshot,
};
use crate::api::studio::types::*;

/// Converts the transport-neutral Studio state into the FRB wire representation.
pub(super) async fn read_studio_state_inner(
    bridge: &'static BridgeRuntime,
) -> Result<BridgeStudioStateSnapshot> {
    let state = bridge.studio.read_state().await?;

    Ok(BridgeStudioStateSnapshot {
        runtime: runtime_snapshot(state.runtime),
        project_directory: bridge_project_directory(state.project_directory.state),
        thread_directory: bridge_thread_directory_page(state.thread_directory.state),
        task_directory: bridge_task_directory(state.task_directory.state),
        agent_directory: bridge_agent_directory(state.agent_directory.state),
        settings: bridge_settings_state(state.settings.state),
        recovery: bridge_recovery_state(state.recovery.state),
        mcp: bridge_mcp_state(state.mcp.state),
        lsp: bridge_lsp_state(state.lsp.state),
        skills_by_project: state
            .skills_by_project
            .into_iter()
            .map(bridge_skills_state)
            .collect(),
        provider_usage: bridge_provider_usage_state(state.provider_usage.state),
        updater: bridge_update_state(state.updater),
        persistence: bridge_persistence_state(state.persistence),
    })
}

/// Reads the complete canonical Studio state without lifecycle side effects.
pub async fn read_studio_state() -> Result<BridgeStudioStateSnapshot, BridgeError> {
    let bridge = super::super::bridge_runtime::active_bridge().await?;
    Ok(read_studio_state_inner(bridge).await?)
}
