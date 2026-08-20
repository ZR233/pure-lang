use anyhow::Result;

use crate::api::studio::bridge_runtime::BridgeRuntime;
use crate::api::studio::convert::runtime::{
    bridge_agent_directory_entry, bridge_mcp_health, bridge_recovery_issue, bridge_task_runtime,
    runtime_snapshot,
};
use crate::api::studio::convert::settings::{bridge_settings, provider_usage_dto};
use crate::api::studio::convert::thread_stream::bridge_thread;
use crate::api::studio::types::*;

/// Converts the transport-neutral Studio state into the FRB wire representation.
pub(super) async fn read_studio_state_inner(
    bridge: &'static BridgeRuntime,
) -> Result<BridgeStudioStateSnapshot> {
    let state = bridge.studio.read_state().await?;

    Ok(BridgeStudioStateSnapshot {
        runtime: runtime_snapshot(state.runtime),
        project_directory: BridgeProjectDirectoryState {
            meta: state.project_directory.meta.into(),
            projects: state
                .project_directory
                .projects
                .into_iter()
                .map(Into::into)
                .collect(),
        },
        thread_directory: BridgeThreadDirectoryPage {
            meta: state.thread_directory.meta.into(),
            threads: state
                .thread_directory
                .threads
                .into_iter()
                .map(bridge_thread)
                .collect(),
            next_cursor: state.thread_directory.next_cursor,
        },
        task_directory: BridgeTaskDirectoryState {
            meta: state.task_directory.meta.into(),
            tasks: state
                .task_directory
                .tasks
                .into_iter()
                .map(|entry| BridgeTaskDirectoryEntry {
                    root_thread_id: entry.root_thread_id,
                    task: bridge_task_runtime(entry.task),
                })
                .collect(),
        },
        agent_directory: BridgeAgentDirectoryState {
            meta: state.agent_directory.meta.into(),
            agents: state
                .agent_directory
                .agents
                .into_iter()
                .map(bridge_agent_directory_entry)
                .collect(),
        },
        settings: BridgeSettingsStateSnapshot {
            meta: state.settings.meta.into(),
            settings: bridge_settings(state.settings.settings),
        },
        recovery: BridgeRecoveryStateSnapshot {
            meta: state.recovery.meta.into(),
            issues: state
                .recovery
                .issues
                .into_iter()
                .map(bridge_recovery_issue)
                .collect(),
        },
        mcp: BridgeMcpStateSnapshot {
            meta: state.mcp.meta.into(),
            desired_config_fingerprint: state.mcp.desired_config_fingerprint,
            applied_config_fingerprint: state.mcp.applied_config_fingerprint,
            health: bridge_mcp_health(state.mcp.health),
        },
        lsp: BridgeLspStateSnapshot {
            meta: state.lsp.meta.into(),
            health: state.lsp.health.into(),
        },
        skills_by_project: state
            .skills_by_project
            .into_iter()
            .map(|skills| BridgeSkillsStateSnapshot {
                meta: skills.meta.into(),
                project_id: skills.project_id,
                config_fingerprint: skills.config_fingerprint,
                catalog_revision: skills.catalog_revision,
                skills: skills
                    .catalog
                    .skills
                    .into_iter()
                    .map(|skill| SkillSummaryDto { name: skill.name })
                    .collect(),
                warnings: skills.catalog.warnings,
            })
            .collect(),
        provider_usage: BridgeProviderUsageStateSnapshot {
            meta: state.provider_usage.meta.into(),
            config_fingerprint: state.provider_usage.config_fingerprint,
            usages: state
                .provider_usage
                .usages
                .into_iter()
                .map(provider_usage_dto)
                .collect(),
        },
        updater: BridgeUpdaterStateSnapshot {
            meta: state.updater.meta.into(),
            update: state
                .updater
                .update
                .map(|update| BridgeVerifiedUpdateSummary {
                    version: update.version,
                    published_at: update.published_at,
                    notes_url: update.notes_url,
                }),
        },
    })
}

/// Reads the complete canonical Studio state without lifecycle side effects.
pub async fn read_studio_state() -> Result<BridgeStudioStateSnapshot, BridgeError> {
    let bridge = super::super::bridge_runtime::active_bridge().await?;
    Ok(read_studio_state_inner(bridge).await?)
}
