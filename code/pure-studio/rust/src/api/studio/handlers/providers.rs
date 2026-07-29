use crate::api::studio::convert::settings::provider_usage_dto;
use crate::api::studio::runtime::active_bridge;
use crate::api::studio::types::{
    BridgeError, ProviderUsagesResponse, SkillSummaryDto, SkillsResponse,
};
// ── Provider usage ──

pub async fn load_provider_usages() -> Result<ProviderUsagesResponse, BridgeError> {
    let bridge = active_bridge().await?;
    let usages = bridge
        .studio
        .provider_usages()
        .await?
        .into_iter()
        .map(provider_usage_dto)
        .collect();
    Ok(ProviderUsagesResponse { usages })
}

pub async fn list_discovered_skills(project_id: String) -> Result<SkillsResponse, BridgeError> {
    let bridge = active_bridge().await?;
    let catalog = bridge.studio.discovered_skills(&project_id).await?;
    Ok(SkillsResponse {
        skills: catalog
            .skills
            .into_iter()
            .map(|skill| SkillSummaryDto { name: skill.name })
            .collect(),
    })
}
