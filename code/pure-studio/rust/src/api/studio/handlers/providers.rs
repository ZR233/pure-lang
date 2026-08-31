use crate::api::studio::bridge_runtime::active_bridge;
use crate::api::studio::convert::runtime::{
    bridge_provider_usage_state, bridge_skill_search_result, bridge_skills_state,
};
use crate::api::studio::types::{
    BridgeError, BridgeProviderUsageStateSnapshot, BridgeSkillSearchResult,
    BridgeSkillsStateSnapshot,
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
    bridge_skills_state(state)
}

pub async fn discover_skills(project_id: String) -> Result<BridgeSkillsStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(skills_state(
        bridge.studio.discover_skills(&project_id).await?,
    ))
}

pub async fn search_skills(
    project_id: String,
    query: String,
    limit: u32,
) -> Result<BridgeSkillSearchResult, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(bridge_skill_search_result(
        bridge
            .studio
            .search_skills(&project_id, &query, limit as usize)
            .await?,
    ))
}

fn provider_usage_state(
    state: pl_studio_runtime::ProviderUsageStateSnapshot,
) -> BridgeProviderUsageStateSnapshot {
    bridge_provider_usage_state(state.state)
}

fn skills_state(state: pl_studio_runtime::SkillsStateSnapshot) -> BridgeSkillsStateSnapshot {
    bridge_skills_state(state.into())
}
