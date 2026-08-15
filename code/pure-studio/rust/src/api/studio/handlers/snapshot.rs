use anyhow::Result;

use crate::api::studio::bridge_runtime::BridgeRuntime;
use crate::api::studio::convert::runtime::{
    bridge_mcp_health, bridge_recovery_issue, bridge_task_runtime, runtime_snapshot,
};
use crate::api::studio::convert::settings::{provider_usage_dto, studio_settings_dto};
use crate::api::studio::convert::thread_stream::bridge_thread;
use crate::api::studio::types::*;

/// 聚合快照携带的 Thread 目录首页大小；后续页由 `listThreadsPage` 按需加载。
const SNAPSHOT_THREAD_PAGE_LIMIT: usize = 50;

/// Studio 聚合纯查询；只组合各 owner 已发布 snapshot 与 SQLite canonical facts。
pub(super) async fn read_studio_state_inner(
    bridge: &'static BridgeRuntime,
) -> Result<BridgeStudioStateSnapshot> {
    let runtime = runtime_snapshot(bridge.studio.runtime_snapshot().await?);
    let project_directory = bridge
        .studio
        .product_events()
        .read_project_directory()
        .await?;
    let thread_directory = bridge
        .studio
        .product_events()
        .read_thread_directory_page(None, SNAPSHOT_THREAD_PAGE_LIMIT)
        .await?;
    let task_directory = bridge.studio.product_events().read_task_directory().await?;
    let agent_directory = bridge.studio.product_events().read_agent_directory().await;
    let recovery_issues = bridge.studio.recovery_issues();
    let settings_state = bridge.studio.settings_state()?;
    let general_settings = general_settings(&settings_state.config);
    let settings = studio_settings_dto(
        &settings_state.config,
        general_settings,
        pl_studio_runtime::StudioRole::Executor,
    )?;
    let mcp_state = bridge.studio.read_mcp_state().await?;
    let lsp_state = bridge.studio.read_lsp_state().await;
    let provider_usage = bridge.studio.read_provider_usage_state().await;
    let updater = bridge.studio.read_update_state().await;
    let mut skills_by_project = Vec::with_capacity(project_directory.projects.len());
    for project in &project_directory.projects {
        let state = bridge
            .studio
            .skill_catalog_runtime()
            .read(&project.id)
            .await;
        skills_by_project.push(BridgeSkillsStateSnapshot {
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
        });
    }

    Ok(BridgeStudioStateSnapshot {
        runtime,
        project_directory: BridgeProjectDirectoryState {
            meta: project_directory.meta.into(),
            projects: project_directory
                .projects
                .into_iter()
                .map(Into::into)
                .collect(),
        },
        thread_directory: BridgeThreadDirectoryPage {
            meta: thread_directory.meta.into(),
            threads: thread_directory
                .threads
                .into_iter()
                .map(bridge_thread)
                .collect(),
            next_cursor: thread_directory.next_cursor,
        },
        task_directory: BridgeTaskDirectoryState {
            meta: task_directory.meta.into(),
            tasks: task_directory
                .tasks
                .into_iter()
                .map(|entry| BridgeTaskDirectoryEntry {
                    root_thread_id: entry.root_thread_id,
                    task: bridge_task_runtime(entry.task),
                })
                .collect(),
        },
        agent_directory: BridgeAgentDirectoryState {
            meta: agent_directory.meta.into(),
            agents: agent_directory
                .agents
                .into_iter()
                .map(crate::api::studio::convert::runtime::bridge_agent_directory_entry)
                .collect(),
        },
        settings: BridgeSettingsStateSnapshot {
            meta: observed_ready(settings_state.revision, settings_state.updated_at),
            settings,
        },
        recovery: BridgeRecoveryStateSnapshot {
            meta: bridge.studio.product_events().recovery_meta().into(),
            issues: recovery_issues
                .into_iter()
                .map(bridge_recovery_issue)
                .collect(),
        },
        mcp: BridgeMcpStateSnapshot {
            meta: mcp_state.meta.into(),
            desired_config_fingerprint: mcp_state.desired_config_fingerprint,
            applied_config_fingerprint: mcp_state.applied_config_fingerprint,
            health: bridge_mcp_health(mcp_state.health),
        },
        lsp: BridgeLspStateSnapshot {
            meta: lsp_state.meta.into(),
            health: lsp_state.health.into(),
        },
        skills_by_project,
        provider_usage: BridgeProviderUsageStateSnapshot {
            meta: provider_usage.meta.into(),
            config_fingerprint: provider_usage.config_fingerprint,
            usages: provider_usage
                .usages
                .into_iter()
                .map(provider_usage_dto)
                .collect(),
        },
        updater: BridgeUpdaterStateSnapshot {
            meta: updater.meta.into(),
            update: updater.update.map(|update| BridgeVerifiedUpdateSummary {
                version: update.version,
                published_at: update.published_at,
                notes_url: update.notes_url,
            }),
        },
    })
}

/// 读取完整 Studio canonical state；不得触发生命周期命令。
pub async fn read_studio_state() -> Result<BridgeStudioStateSnapshot, BridgeError> {
    let bridge = super::super::bridge_runtime::active_bridge().await?;
    Ok(read_studio_state_inner(bridge).await?)
}

pub(super) fn settings_snapshot(
    state: &pl_studio_runtime::ConfigRuntimeSnapshot,
) -> Result<BridgeSettingsStateSnapshot> {
    Ok(BridgeSettingsStateSnapshot {
        meta: observed_ready(state.revision, state.updated_at),
        settings: studio_settings_dto(
            &state.config,
            general_settings(&state.config),
            pl_studio_runtime::StudioRole::Executor,
        )?,
    })
}

pub(crate) fn general_settings(
    config: &pl_studio_runtime::StudioConfig,
) -> BridgeGeneralSettingsDto {
    BridgeGeneralSettingsDto {
        follow_system_theme: config.ui.follow_system_theme,
        follow_active_turn: config.ui.follow_active_turn,
        compact_timeline: config.ui.compact_timeline,
    }
}

pub(super) fn ensure_project_recovery_available(
    bridge: &'static BridgeRuntime,
    project_id: &str,
) -> Result<()> {
    if bridge.studio.recovery_issues().iter().any(|issue| {
        issue.scope == pl_studio_runtime::StudioRecoveryIssueScope::Project
            && issue.project_id.as_deref() == Some(project_id)
    }) {
        anyhow::bail!("selected project is blocked by a recovery issue");
    }
    Ok(())
}

fn observed_ready(revision: u64, updated_at: i64) -> BridgeObservedStateMeta {
    pl_protocol::ObservedStateMeta::ready(revision, updated_at).into()
}
