use anyhow::Result;
use pl_studio_runtime::{
    StudioProductBaseline, StudioProductBaselineState, StudioProductEventEnvelope,
    StudioProductEventKind,
};

use super::runtime::{
    bridge_agent_directory, bridge_agent_profiles, bridge_lsp_state, bridge_mcp_state,
    bridge_model_catalog_state, bridge_model_performance, bridge_persistence_queue_state,
    bridge_persistence_state, bridge_project_directory, bridge_provider_usage_state,
    bridge_recovery_state, bridge_session_costs_state, bridge_settings_config_state,
    bridge_skills_state, bridge_thread_mode_catalog, bridge_update_state,
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
            StudioProductEventKind::AgentDirectoryChanged(state) => {
                BridgeProductEventPayload::AgentDirectoryChanged(bridge_agent_directory(
                    state.state,
                ))
            }
            StudioProductEventKind::SettingsConfigStateChanged(state) => {
                BridgeProductEventPayload::SettingsConfigStateChanged(Box::new(
                    bridge_settings_config_state(state.state),
                ))
            }
            StudioProductEventKind::ModelCatalogStateChanged(state) => {
                BridgeProductEventPayload::ModelCatalogStateChanged(Box::new(
                    bridge_model_catalog_state(state.state),
                ))
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
            StudioProductEventKind::ThreadModeCatalogChanged(state) => {
                BridgeProductEventPayload::ThreadModeCatalogChanged(bridge_thread_mode_catalog(
                    state,
                ))
            }
            StudioProductEventKind::ProviderUsageStateChanged(state) => {
                BridgeProductEventPayload::ProviderUsageStateChanged(bridge_provider_usage_state(
                    state.state,
                ))
            }
            StudioProductEventKind::ModelPerformanceStateChanged(state) => {
                BridgeProductEventPayload::ModelPerformanceStateChanged(bridge_model_performance(
                    state,
                ))
            }
            StudioProductEventKind::SessionCostsChanged(state) => {
                BridgeProductEventPayload::SessionCostsChanged(Box::new(
                    bridge_session_costs_state(state),
                ))
            }
            StudioProductEventKind::UpdaterStateChanged(state) => {
                BridgeProductEventPayload::UpdaterStateChanged(bridge_update_state(state))
            }
            StudioProductEventKind::PersistenceStateChanged(state) => {
                BridgeProductEventPayload::PersistenceStateChanged(bridge_persistence_state(state))
            }
            StudioProductEventKind::PersistenceQueueStateChanged(state) => {
                BridgeProductEventPayload::PersistenceQueueStateChanged(Box::new(
                    bridge_persistence_queue_state(state),
                ))
            }
            StudioProductEventKind::AgentProfilesStateChanged(state) => {
                BridgeProductEventPayload::AgentProfilesStateChanged(Box::new(
                    BridgeAgentProfilesStateSnapshot {
                        state: bridge_agent_profiles(state.state),
                    },
                ))
            }
        },
    })
}

/// 把 runtime 的 typed 基线转换为 FRB 基线 payload。
pub(crate) fn bridge_product_baseline(
    baseline: StudioProductBaseline,
) -> Result<(u64, BridgeProductBaseline)> {
    let revision = baseline.revision;
    let state = match baseline.state {
        StudioProductBaselineState::ProjectDirectory(state) => {
            BridgeProductBaseline::ProjectDirectory(bridge_project_directory(state.state))
        }
        StudioProductBaselineState::ThreadDirectory(state) => {
            BridgeProductBaseline::ThreadDirectory(super::runtime::bridge_thread_directory_page(
                state.state,
            ))
        }
        StudioProductBaselineState::AgentDirectory(state) => {
            BridgeProductBaseline::AgentDirectory(bridge_agent_directory(state.state))
        }
        StudioProductBaselineState::SettingsConfig(state) => BridgeProductBaseline::SettingsConfig(
            Box::new(bridge_settings_config_state(state.state)),
        ),
        StudioProductBaselineState::ModelCatalog(state) => {
            BridgeProductBaseline::ModelCatalog(Box::new(bridge_model_catalog_state(state.state)))
        }
        StudioProductBaselineState::Recovery(state) => {
            BridgeProductBaseline::Recovery(bridge_recovery_state(state.state))
        }
        StudioProductBaselineState::Mcp(state) => {
            BridgeProductBaseline::Mcp(bridge_mcp_state(state.state))
        }
        StudioProductBaselineState::Lsp(state) => {
            BridgeProductBaseline::Lsp(bridge_lsp_state(state.state))
        }
        StudioProductBaselineState::Skills(state) => {
            BridgeProductBaseline::Skills(bridge_skills_state(*state))
        }
        StudioProductBaselineState::ThreadModeCatalog(state) => {
            BridgeProductBaseline::ThreadModeCatalog(bridge_thread_mode_catalog(*state))
        }
        StudioProductBaselineState::ProviderUsage(state) => {
            BridgeProductBaseline::ProviderUsage(bridge_provider_usage_state(state.state))
        }
        StudioProductBaselineState::ModelPerformance(state) => {
            BridgeProductBaseline::ModelPerformance(bridge_model_performance(*state))
        }
        StudioProductBaselineState::SessionCosts(state) => {
            BridgeProductBaseline::SessionCosts(Box::new(bridge_session_costs_state(*state)))
        }
        StudioProductBaselineState::Updater(state) => {
            BridgeProductBaseline::Updater(bridge_update_state(*state))
        }
        StudioProductBaselineState::Persistence(state) => {
            BridgeProductBaseline::Persistence(bridge_persistence_state(*state))
        }
        StudioProductBaselineState::PersistenceQueue(state) => {
            BridgeProductBaseline::PersistenceQueue(Box::new(bridge_persistence_queue_state(
                *state,
            )))
        }
        StudioProductBaselineState::AgentProfiles(state) => {
            BridgeProductBaseline::AgentProfiles(Box::new(BridgeAgentProfilesStateSnapshot {
                state: bridge_agent_profiles(state.state),
            }))
        }
    };
    Ok((revision, state))
}
