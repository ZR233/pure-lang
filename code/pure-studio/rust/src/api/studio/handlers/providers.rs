use crate::api::studio::bridge_runtime::active_bridge;
use crate::api::studio::convert::settings::provider_usage_dto;
use crate::api::studio::types::{
    BridgeError, BridgeProviderUsageStateSnapshot, BridgeSkillsStateSnapshot, SkillSummaryDto,
};
// ── Provider usage ──

pub async fn read_provider_usage_state() -> Result<BridgeProviderUsageStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(provider_usage_state(
        bridge.studio.read_provider_usage_state().await,
    ))
}

pub async fn check_provider_usage() -> Result<BridgeProviderUsageStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(provider_usage_state(
        bridge.studio.check_provider_usage().await?,
    ))
}

pub async fn read_skills_state(
    project_id: String,
) -> Result<BridgeSkillsStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(owned_skills_state(
        bridge.studio.read_skills_state(&project_id).await,
    ))
}

fn owned_skills_state(
    state: pl_studio_runtime::StudioSkillsStateSnapshot,
) -> BridgeSkillsStateSnapshot {
    BridgeSkillsStateSnapshot {
        meta: state.meta.into(),
        project_id: state.project_id,
        config_fingerprint: state.config_fingerprint,
        catalog_revision: state.catalog_revision,
        skills: state
            .catalog
            .skills
            .into_iter()
            .map(|skill| SkillSummaryDto { name: skill.name })
            .collect(),
        warnings: state.catalog.warnings,
    }
}

pub async fn discover_skills(project_id: String) -> Result<BridgeSkillsStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(skills_state(
        bridge.studio.discover_skills(&project_id).await?,
    ))
}

fn provider_usage_state(
    state: pl_studio_runtime::ProviderUsageStateSnapshot,
) -> BridgeProviderUsageStateSnapshot {
    BridgeProviderUsageStateSnapshot {
        meta: state.meta.into(),
        config_fingerprint: state.config_fingerprint,
        usages: state.usages.into_iter().map(provider_usage_dto).collect(),
    }
}

fn skills_state(state: pl_studio_runtime::SkillsStateSnapshot) -> BridgeSkillsStateSnapshot {
    BridgeSkillsStateSnapshot {
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
    }
}
