//! `ObservedResource<T>` 到各 FRB concrete union 的穷尽转换。

use pl_protocol::ObservedResource;
use pl_studio_runtime::*;

use crate::api::studio::convert::settings::{bridge_settings, provider_usage_dto};
use crate::api::studio::convert::thread_stream::bridge_thread;
use crate::api::studio::types::*;

use super::{
    bridge_agent_directory_entry, bridge_degraded_resource, bridge_failed_resource,
    bridge_loading_resource, bridge_mcp_health, bridge_ready_resource, bridge_recovery_issue,
    bridge_refreshing_resource, bridge_stale_resource, bridge_stopped_resource,
    bridge_uninitialized_resource,
};

enum BridgeObservedValue<T> {
    Uninitialized(BridgeUninitializedResource),
    Loading(BridgeLoadingResource),
    Ready(BridgeReadyResource, T),
    Refreshing(BridgeRefreshingResource, T),
    Stale(BridgeStaleResource, T),
    Degraded(BridgeDegradedResource, T),
    Failed(BridgeFailedResource),
    Stopped(BridgeStoppedResource),
}

fn bridge_observed<T, U>(
    state: ObservedResource<T>,
    map_value: impl Fn(T) -> U,
) -> BridgeObservedValue<U>
where
    T: Clone,
{
    match state {
        ObservedResource::Uninitialized(state) => {
            BridgeObservedValue::Uninitialized(bridge_uninitialized_resource(&state))
        }
        ObservedResource::Loading(state) => {
            BridgeObservedValue::Loading(bridge_loading_resource(&state))
        }
        ObservedResource::Ready(state) => BridgeObservedValue::Ready(
            bridge_ready_resource(&state),
            map_value(state.value().clone()),
        ),
        ObservedResource::Refreshing(state) => BridgeObservedValue::Refreshing(
            bridge_refreshing_resource(&state),
            map_value(state.value().clone()),
        ),
        ObservedResource::Stale(state) => BridgeObservedValue::Stale(
            bridge_stale_resource(&state),
            map_value(state.value().clone()),
        ),
        ObservedResource::Degraded(state) => BridgeObservedValue::Degraded(
            bridge_degraded_resource(&state),
            map_value(state.value().clone()),
        ),
        ObservedResource::Failed(state) => {
            BridgeObservedValue::Failed(bridge_failed_resource(&state))
        }
        ObservedResource::Stopped(state) => {
            BridgeObservedValue::Stopped(bridge_stopped_resource(&state))
        }
    }
}

macro_rules! concrete_observed {
    ($state:expr, $map_value:expr, $target:ident) => {
        match bridge_observed($state, $map_value) {
            BridgeObservedValue::Uninitialized(resource) => $target::Uninitialized(resource),
            BridgeObservedValue::Loading(resource) => $target::Loading(resource),
            BridgeObservedValue::Ready(resource, value) => $target::Ready { resource, value },
            BridgeObservedValue::Refreshing(resource, value) => {
                $target::Refreshing { resource, value }
            }
            BridgeObservedValue::Stale(resource, value) => $target::Stale { resource, value },
            BridgeObservedValue::Degraded(resource, value) => $target::Degraded { resource, value },
            BridgeObservedValue::Failed(resource) => $target::Failed(resource),
            BridgeObservedValue::Stopped(resource) => $target::Stopped(resource),
        }
    };
}

pub(crate) fn bridge_project_directory(
    state: ObservedResource<StudioProjectDirectoryData>,
) -> BridgeProjectDirectoryState {
    concrete_observed!(
        state,
        |data| BridgeProjectDirectoryData {
            projects: data.projects.into_iter().map(Into::into).collect(),
        },
        BridgeProjectDirectoryState
    )
}

pub(crate) fn bridge_thread_directory_page(
    state: ObservedResource<StudioThreadDirectoryPageData>,
) -> BridgeThreadDirectoryPage {
    concrete_observed!(
        state,
        |data| BridgeThreadDirectoryPageData {
            threads: data.threads.into_iter().map(bridge_thread).collect(),
            next_cursor: data.next_cursor,
        },
        BridgeThreadDirectoryPage
    )
}

pub(crate) fn bridge_agent_directory(
    state: ObservedResource<StudioAgentDirectoryData>,
) -> BridgeAgentDirectoryState {
    concrete_observed!(
        state,
        |data| BridgeAgentDirectoryData {
            agents: data
                .agents
                .into_iter()
                .map(bridge_agent_directory_entry)
                .collect(),
        },
        BridgeAgentDirectoryState
    )
}

pub(crate) fn bridge_settings_state(
    state: ObservedResource<pl_protocol::studio::StudioSettings>,
) -> BridgeSettingsStateSnapshot {
    concrete_observed!(
        state,
        |settings| BridgeSettingsStateData {
            settings: bridge_settings(settings),
        },
        BridgeSettingsStateSnapshot
    )
}

pub(crate) fn bridge_recovery_state(
    state: ObservedResource<Vec<StudioRecoveryIssue>>,
) -> BridgeRecoveryStateSnapshot {
    concrete_observed!(
        state,
        |issues| BridgeRecoveryStateData {
            issues: issues.into_iter().map(bridge_recovery_issue).collect(),
        },
        BridgeRecoveryStateSnapshot
    )
}

pub(crate) fn bridge_mcp_state(
    state: ObservedResource<StudioMcpStateData>,
) -> BridgeMcpStateSnapshot {
    concrete_observed!(
        state,
        |data| BridgeMcpStateData {
            desired_config_fingerprint: data.desired_config_fingerprint,
            applied_config_fingerprint: data.applied_config_fingerprint,
            health: bridge_mcp_health(data.health),
        },
        BridgeMcpStateSnapshot
    )
}

pub(crate) fn bridge_lsp_state(state: ObservedResource<StudioLspHealth>) -> BridgeLspStateSnapshot {
    concrete_observed!(
        state,
        |health| BridgeLspStateData {
            health: health.into(),
        },
        BridgeLspStateSnapshot
    )
}

pub(crate) fn bridge_skills_state(state: StudioSkillsStateSnapshot) -> BridgeSkillsStateSnapshot {
    BridgeSkillsStateSnapshot {
        project_id: state.project_id,
        state: concrete_observed!(
            state.state,
            |data| BridgeSkillsStateData {
                config_fingerprint: data.config_fingerprint,
                catalog_revision: data.catalog_revision,
                skills: data.catalog.skills.into_iter().map(skill_summary).collect(),
                warnings: data.catalog.warnings,
                complete: data.catalog.complete,
            },
            BridgeSkillsResourceState
        ),
    }
}

pub(crate) fn bridge_skill_search_result(
    result: pl_studio_runtime::SkillSearchResult,
) -> BridgeSkillSearchResult {
    BridgeSkillSearchResult {
        project_id: result.project_id,
        catalog_revision: result.catalog_revision,
        matches: result
            .matches
            .into_iter()
            .map(Into::into)
            .map(skill_summary)
            .collect(),
        truncated: result.truncated,
    }
}

pub(crate) fn bridge_thread_mode_catalog(
    catalog: pl_protocol::ThreadModeCatalogSnapshot,
) -> BridgeThreadModeCatalogSnapshot {
    BridgeThreadModeCatalogSnapshot {
        revision: catalog.revision,
        modes: catalog
            .modes
            .into_iter()
            .map(|mode_| BridgeThreadModeDescriptor {
                id: mode_.id.to_string(),
                display_name: mode_.display_name,
                description: mode_.description,
                order: mode_.order,
                has_workflow: mode_.has_workflow,
            })
            .collect(),
    }
}

fn skill_summary(skill: pl_core::skill::SkillMetadata) -> SkillSummaryDto {
    SkillSummaryDto {
        name: skill.name,
        description: skill.description,
        category: skill.category,
        platforms: skill.platforms,
        source: match skill.source {
            pl_core::skill::SkillSourceKind::Project => "project",
            pl_core::skill::SkillSourceKind::User => "user",
            pl_core::skill::SkillSourceKind::System => "system",
            pl_core::skill::SkillSourceKind::External => "external",
        }
        .to_string(),
        provider_id: skill.provider_id.as_str().to_string(),
        invocation: SkillInvocationPolicyDto {
            model_invocable: skill.invocation.model_invocable,
            user_invocable: skill.invocation.user_invocable,
        },
        resource_base: match skill.resource_base {
            pl_core::skill::SkillResourceBase::Directory { path } => {
                SkillResourceBaseDto::Directory {
                    path: path.to_string_lossy().to_string(),
                }
            }
            pl_core::skill::SkillResourceBase::Url { url } => SkillResourceBaseDto::Url { url },
            pl_core::skill::SkillResourceBase::Opaque { description } => {
                SkillResourceBaseDto::Opaque { description }
            }
        },
    }
}

pub(crate) fn bridge_provider_usage_state(
    state: ObservedResource<ProviderUsageStateData>,
) -> BridgeProviderUsageStateSnapshot {
    concrete_observed!(
        state,
        |data| BridgeProviderUsageStateData {
            config_fingerprint: data.config_fingerprint,
            usages: data.usages.into_iter().map(provider_usage_dto).collect(),
        },
        BridgeProviderUsageStateSnapshot
    )
}
