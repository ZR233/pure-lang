use anyhow::Result;
use pl_studio_runtime::{StudioProductEventEnvelope, StudioProductEventKind};

use super::runtime::{
    bridge_agent_directory_entry, bridge_mcp_health, bridge_recovery_issue, bridge_task_runtime,
};
use super::settings::{provider_usage_dto, studio_settings_dto};
use super::thread_stream::bridge_thread;
use crate::api::studio::handlers::snapshot::general_settings;
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
                BridgeProductEventPayload::ProjectDirectoryChanged(BridgeProjectDirectoryState {
                    meta: state.meta.into(),
                    projects: state.projects.into_iter().map(Into::into).collect(),
                })
            }
            StudioProductEventKind::ThreadDirectoryChanged(state) => {
                BridgeProductEventPayload::ThreadDirectoryChanged(BridgeThreadDirectoryState {
                    meta: state.meta.into(),
                    threads: state.threads.into_iter().map(bridge_thread).collect(),
                })
            }
            StudioProductEventKind::TaskDirectoryChanged(state) => {
                BridgeProductEventPayload::TaskDirectoryChanged(BridgeTaskDirectoryState {
                    meta: state.meta.into(),
                    tasks: state
                        .tasks
                        .into_iter()
                        .map(|entry| BridgeTaskDirectoryEntry {
                            root_thread_id: entry.root_thread_id,
                            task: bridge_task_runtime(entry.task),
                        })
                        .collect(),
                })
            }
            StudioProductEventKind::AgentDirectoryChanged(state) => {
                BridgeProductEventPayload::AgentDirectoryChanged(BridgeAgentDirectoryState {
                    meta: state.meta.into(),
                    agents: state
                        .agents
                        .into_iter()
                        .map(bridge_agent_directory_entry)
                        .collect(),
                })
            }
            StudioProductEventKind::SettingsStateChanged(state) => {
                let settings = studio_settings_dto(
                    &state.settings.config,
                    general_settings(&state.settings.config),
                    pl_studio_runtime::StudioRole::Executor,
                )?;
                BridgeProductEventPayload::SettingsStateChanged(Box::new(
                    BridgeSettingsStateSnapshot {
                        meta: state.meta.into(),
                        settings,
                    },
                ))
            }
            StudioProductEventKind::RecoveryStateChanged(state) => {
                BridgeProductEventPayload::RecoveryStateChanged(BridgeRecoveryStateSnapshot {
                    meta: state.meta.into(),
                    issues: state
                        .issues
                        .into_iter()
                        .map(bridge_recovery_issue)
                        .collect(),
                })
            }
            StudioProductEventKind::McpStateChanged(state) => {
                BridgeProductEventPayload::McpStateChanged(BridgeMcpStateSnapshot {
                    meta: state.meta.into(),
                    desired_config_fingerprint: state.desired_config_fingerprint,
                    applied_config_fingerprint: state.applied_config_fingerprint,
                    health: bridge_mcp_health(state.health),
                })
            }
            StudioProductEventKind::LspStateChanged(state) => {
                BridgeProductEventPayload::LspStateChanged(BridgeLspStateSnapshot {
                    meta: state.meta.into(),
                    health: state.health.into(),
                })
            }
            StudioProductEventKind::SkillsStateChanged(state) => {
                BridgeProductEventPayload::SkillsStateChanged(BridgeSkillsStateSnapshot {
                    meta: state.meta.into(),
                    project_id: state.project_id,
                    config_fingerprint: state.config_fingerprint,
                    catalog_revision: state.catalog_revision,
                    skills: state
                        .catalog
                        .skills
                        .iter()
                        .map(|skill| SkillSummaryDto {
                            name: skill.name.clone(),
                        })
                        .collect(),
                    warnings: state.catalog.warnings.clone(),
                })
            }
            StudioProductEventKind::ProviderUsageStateChanged(state) => {
                BridgeProductEventPayload::ProviderUsageStateChanged(
                    BridgeProviderUsageStateSnapshot {
                        meta: state.meta.into(),
                        config_fingerprint: state.config_fingerprint,
                        usages: state.usages.into_iter().map(provider_usage_dto).collect(),
                    },
                )
            }
            StudioProductEventKind::UpdaterStateChanged(state) => {
                BridgeProductEventPayload::UpdaterStateChanged(BridgeUpdaterStateSnapshot {
                    meta: state.meta.into(),
                    update: state.update.map(|update| BridgeVerifiedUpdateSummary {
                        version: update.version,
                        published_at: update.published_at,
                        notes_url: update.notes_url,
                    }),
                })
            }
        },
    })
}
