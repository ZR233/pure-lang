use crate::api::studio::bridge_runtime::{active_bridge, installed_bridge};
use crate::api::studio::convert::settings::{
    bridge_settings_snapshot, bridge_web_search_settings, provider_settings_request,
};
use crate::api::studio::types::{
    BridgeError, BridgeProviderCatalogSnapshot, BridgeSettingsStateSnapshot,
    BridgeWebSearchSettingsDto, InstructionsSettingsInput, McpSettingsInput, ProviderSettingsInput,
    SkillsSettingsInput, WebSearchSettingsInput,
};
// ── Settings ──

pub fn load_provider_catalog() -> Result<BridgeProviderCatalogSnapshot, BridgeError> {
    Ok(installed_bridge()?.studio.load_provider_catalog()?.into())
}

pub async fn read_web_search_settings() -> Result<BridgeWebSearchSettingsDto, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(bridge_web_search_settings(
        bridge.studio.read_settings()?.settings.web_search,
    ))
}

pub async fn save_web_search_settings(
    expected_settings_revision: u64,
    input: WebSearchSettingsInput,
) -> Result<BridgeSettingsStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    let snapshot = bridge.studio.save_web_search_settings(
        pl_protocol::studio::UpdateWebSearchSettingsRequest {
            expected_revision: expected_settings_revision,
            mode: input.mode,
            context_size: input.context_size,
            allowed_domains: input.allowed_domains,
            country: input.country,
            region: input.region,
            city: input.city,
            timezone: input.timezone,
        },
    )?;
    Ok(bridge_settings_snapshot(snapshot))
}

pub async fn read_settings_state() -> Result<BridgeSettingsStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(bridge_settings_snapshot(bridge.studio.read_settings()?))
}

pub async fn reload_settings_from_disk(
    expected_settings_revision: u64,
) -> Result<BridgeSettingsStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(bridge_settings_snapshot(
        bridge
            .studio
            .reload_settings(expected_settings_revision)
            .await?,
    ))
}

pub async fn save_runtime_permission_mode(
    expected_settings_revision: u64,
    mode: String,
) -> Result<BridgeSettingsStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(bridge_settings_snapshot(
        bridge.studio.save_permission_settings(
            pl_protocol::studio::UpdatePermissionSettingsRequest {
                expected_revision: expected_settings_revision,
                mode,
            },
        )?,
    ))
}

pub async fn save_provider_settings(
    expected_settings_revision: u64,
    input: ProviderSettingsInput,
) -> Result<BridgeSettingsStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(bridge_settings_snapshot(
        bridge
            .studio
            .save_provider_settings(provider_settings_request(expected_settings_revision, input))
            .await?,
    ))
}

pub async fn save_instructions_settings(
    expected_settings_revision: u64,
    input: InstructionsSettingsInput,
) -> Result<BridgeSettingsStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(bridge_settings_snapshot(
        bridge.studio.save_instructions_settings(
            pl_protocol::studio::UpdateInstructionsSettingsRequest {
                expected_revision: expected_settings_revision,
                settings: pl_protocol::studio::StudioInstructionsSettings {
                    base_override: input.base_override,
                    developer: input.developer,
                    user: input.user,
                    project_doc_max_bytes: input.project_doc_max_bytes as u64,
                    project_doc_fallback_filenames: input.project_doc_fallback_filenames,
                },
            },
        )?,
    ))
}

pub async fn save_skills_settings(
    expected_settings_revision: u64,
    input: SkillsSettingsInput,
) -> Result<BridgeSettingsStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(bridge_settings_snapshot(
        bridge
            .studio
            .save_skills_settings(pl_protocol::studio::UpdateSkillsSettingsRequest {
                expected_revision: expected_settings_revision,
                settings: pl_protocol::studio::StudioSkillsSettings {
                    enabled: input.enabled,
                    auto_learn: input.auto_learn,
                    system_enabled: input.system_enabled,
                    project_dir: input.project_dir,
                    user_dir: input.user_dir,
                    external_dirs: input.external_dirs,
                    disabled: input.disabled,
                    auto_learn_min_tool_calls: input.auto_learn_min_tool_calls,
                },
            })
            .await?,
    ))
}

pub async fn save_mcp_settings(
    expected_settings_revision: u64,
    input: McpSettingsInput,
) -> Result<BridgeSettingsStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(bridge_settings_snapshot(
        bridge
            .studio
            .save_mcp_settings(pl_protocol::studio::UpdateMcpSettingsRequest {
                expected_revision: expected_settings_revision,
                servers: input
                    .servers
                    .into_iter()
                    .map(|server| pl_protocol::studio::McpServerUpdate {
                        id: server.id,
                        enabled: server.enabled,
                        transport: server.transport,
                        endpoint: server.endpoint,
                    })
                    .collect(),
            })
            .await?,
    ))
}

pub async fn save_general_settings(
    expected_settings_revision: u64,
    input: crate::api::studio::types::GeneralSettingsInput,
) -> Result<BridgeSettingsStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(bridge_settings_snapshot(
        bridge
            .studio
            .save_general_settings(pl_protocol::studio::UpdateGeneralSettingsRequest {
                expected_revision: expected_settings_revision,
                settings: pl_protocol::studio::StudioGeneralSettings {
                    follow_system_theme: input.follow_system_theme,
                    follow_active_turn: input.follow_active_turn,
                    compact_timeline: input.compact_timeline,
                },
            })?,
    ))
}

pub async fn set_model_role(
    expected_settings_revision: u64,
    role_key: String,
    provider_id: String,
    model: String,
    effort: Option<String>,
) -> Result<BridgeSettingsStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(bridge_settings_snapshot(bridge.studio.save_model_role(
        pl_protocol::studio::SetModelRoleRequest {
            expected_revision: expected_settings_revision,
            role: role_key,
            provider_id,
            model,
            effort,
        },
    )?))
}
