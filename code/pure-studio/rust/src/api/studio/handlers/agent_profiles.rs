use crate::api::studio::bridge_runtime::active_bridge;
use crate::api::studio::convert::settings::bridge_settings_snapshot;
use crate::api::studio::types::{
    BridgeAgentProfileDto, BridgeAgentWorkspaceMode, BridgeError, BridgeSettingsStateSnapshot,
};

/// Reads every valid Agent Profile for settings, including disabled profiles.
/// Built-in profiles are immutable and marked with `system: true`.
pub async fn read_agent_profiles() -> Result<Vec<BridgeAgentProfileDto>, BridgeError> {
    let bridge = active_bridge().await?;
    let catalog = bridge.studio.read_agent_profiles()?;
    Ok(catalog
        .profiles
        .into_iter()
        .map(|profile| BridgeAgentProfileDto {
            profile_id: profile.profile_id,
            display_name: profile.display_name,
            description: profile.description,
            when_to_use: profile.when_to_use,
            system_instructions: profile.system_instructions,
            provider_id: profile.provider_id,
            model: profile.model,
            effort: profile.effort,
            source: profile.source,
            revision: profile.revision,
            content_hash: profile.content_hash,
            system: profile.system,
            enabled: profile.enabled,
            workspace_mode: profile.workspace_mode.into(),
        })
        .collect())
}

/// Enables or disables an immutable built-in Agent Profile.
pub async fn set_system_agent_enabled(
    expected_settings_revision: u64,
    profile_id: String,
    enabled: bool,
) -> Result<BridgeSettingsStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(bridge_settings_snapshot(
        bridge
            .studio
            .set_system_agent_enabled(expected_settings_revision, &profile_id, enabled)?,
    ))
}

/// Atomically creates or replaces one user Agent Profile TOML file.
#[allow(clippy::too_many_arguments)]
pub async fn save_user_agent_profile(
    expected_settings_revision: u64,
    profile_id: String,
    enabled: bool,
    display_name: String,
    description: String,
    when_to_use: String,
    system_instructions: String,
    provider_id: String,
    model: String,
    effort: Option<String>,
    workspace_mode: BridgeAgentWorkspaceMode,
) -> Result<BridgeSettingsStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    let profile = pl_studio_runtime::UserAgentProfile {
        enabled,
        display_name,
        description,
        when_to_use,
        system_instructions,
        provider: pl_studio_runtime::ProviderId::new(provider_id)?,
        model,
        effort: effort
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .map(pl_studio_runtime::ReasoningEffort::new),
        workspace_mode: match workspace_mode {
            BridgeAgentWorkspaceMode::Unrestricted => pl_protocol::AgentWorkspaceMode::Unrestricted,
            BridgeAgentWorkspaceMode::Directory => pl_protocol::AgentWorkspaceMode::Directory,
            BridgeAgentWorkspaceMode::Worktree => pl_protocol::AgentWorkspaceMode::Worktree,
        },
    };
    Ok(bridge_settings_snapshot(
        bridge
            .studio
            .save_user_agent_profile(expected_settings_revision, &profile_id, &profile)?,
    ))
}

/// Explicitly removes one revision-matched preserved Pure worktree and branch.
pub async fn cleanup_preserved_worktree(
    child_id: String,
    expected_lease_revision: u64,
) -> Result<(), BridgeError> {
    let bridge = active_bridge().await?;
    bridge
        .studio
        .cleanup_preserved_worktree(&child_id, expected_lease_revision)
        .await?;
    Ok(())
}
