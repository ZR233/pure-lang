use crate::api::studio::convert::settings::provider_usage_dto;
use crate::api::studio::runtime::bridge;
use crate::api::studio::types::{ProviderUsagesResponse, SkillSummaryDto, SkillsResponse};
use anyhow::Result;
// ── Provider usage ──

pub fn load_provider_usages() -> Result<ProviderUsagesResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let usages = bridge
            .studio
            .provider_usages()
            .await?
            .into_iter()
            .map(provider_usage_dto)
            .collect();
        Ok(ProviderUsagesResponse { usages })
    })
}

pub fn list_discovered_skills(project_id: String) -> Result<SkillsResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let catalog = bridge.studio.discovered_skills(&project_id).await?;
        Ok(SkillsResponse {
            skills: catalog
                .skills
                .into_iter()
                .map(|skill| SkillSummaryDto { name: skill.name })
                .collect(),
        })
    })
}
