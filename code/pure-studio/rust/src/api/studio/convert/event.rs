use anyhow::Result;
use pl_studio_runtime::{StudioProductEventEnvelope, StudioProductEventKind};

use super::runtime::{
    bridge_agent_directory, bridge_lsp_state, bridge_mcp_state, bridge_persistence_state,
    bridge_project_directory, bridge_provider_usage_state, bridge_recovery_state,
    bridge_settings_state, bridge_skills_state, bridge_task_directory, bridge_update_state,
};
use super::thread_stream::bridge_thread;
use crate::api::studio::types::*;

pub(crate) fn bridge_product_event(
    event: StudioProductEventEnvelope,
) -> Result<BridgeProductEventEnvelope> {
    Ok(BridgeProductEventEnvelope {
        event_id: event.event_id,
        sequence: event.sequence,
        created_at: event.created_at,
        payload: match event.kind {
            StudioProductEventKind::ProjectDirectoryChanged(state) => {
                BridgeProductEventPayload::ProjectDirectoryChanged(bridge_project_directory(
                    state.state,
                ))
            }
            StudioProductEventKind::ThreadDirectoryChanged(state) => {
                BridgeProductEventPayload::ThreadDirectoryChanged(BridgeThreadDirectoryDelta {
                    revision: state.revision,
                    updated_at: state.updated_at,
                    upserted: state.upserted.into_iter().map(bridge_thread).collect(),
                    removed: state.removed,
                })
            }
            StudioProductEventKind::TaskDirectoryChanged(state) => {
                BridgeProductEventPayload::TaskDirectoryChanged(bridge_task_directory(state.state))
            }
            StudioProductEventKind::AgentDirectoryChanged(state) => {
                BridgeProductEventPayload::AgentDirectoryChanged(bridge_agent_directory(
                    state.state,
                ))
            }
            StudioProductEventKind::SettingsStateChanged(state) => {
                BridgeProductEventPayload::SettingsStateChanged(Box::new(bridge_settings_state(
                    state.state,
                )))
            }
            StudioProductEventKind::RecoveryStateChanged(state) => {
                BridgeProductEventPayload::RecoveryStateChanged(bridge_recovery_state(state.state))
            }
            StudioProductEventKind::McpStateChanged(state) => {
                BridgeProductEventPayload::McpStateChanged(bridge_mcp_state(state.state))
            }
            StudioProductEventKind::LspStateChanged(state) => {
                BridgeProductEventPayload::LspStateChanged(bridge_lsp_state(state.state))
            }
            StudioProductEventKind::SkillsStateChanged(state) => {
                BridgeProductEventPayload::SkillsStateChanged(bridge_skills_state(state))
            }
            StudioProductEventKind::ProviderUsageStateChanged(state) => {
                BridgeProductEventPayload::ProviderUsageStateChanged(bridge_provider_usage_state(
                    state.state,
                ))
            }
            StudioProductEventKind::UpdaterStateChanged(state) => {
                BridgeProductEventPayload::UpdaterStateChanged(bridge_update_state(state))
            }
            StudioProductEventKind::PersistenceStateChanged(state) => {
                BridgeProductEventPayload::PersistenceStateChanged(bridge_persistence_state(state))
            }
        },
    })
}
