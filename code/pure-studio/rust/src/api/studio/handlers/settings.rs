use super::snapshot::studio_snapshot_inner;
use crate::api::studio::convert::settings::{
    mcp_transport_from_label, normalized_string_list, provider_settings_edit,
    web_search_config_from_input, web_search_settings_dto,
};
use crate::api::studio::runtime::active_bridge;
use crate::api::studio::types::{
    BridgeError, BridgeProviderCatalogSnapshot, BridgeStudioSnapshotResponse,
    BridgeWebSearchSettingsDto, InstructionsSettingsInput, McpSettingsInput, ProviderSettingsInput,
    SkillsSettingsInput, WebSearchSettingsInput,
};
use pl_studio_runtime::{
    BuiltinMcpServerState, McpServerConfig, McpServerTransport, PermissionMode,
    is_builtin_mcp_server_id,
};
// ── Settings ──

pub fn load_provider_catalog() -> Result<BridgeProviderCatalogSnapshot, BridgeError> {
    Ok(pl_studio_runtime::builtin_provider_catalog()
        .snapshot()?
        .into())
}

pub async fn load_web_search_settings() -> Result<BridgeWebSearchSettingsDto, BridgeError> {
    let bridge = active_bridge().await?;
    let config = bridge.studio.config_store().load_or_default()?;
    Ok(web_search_settings_dto(
        &config,
        pl_studio_runtime::StudioRole::Executor,
    )?)
}

pub async fn save_web_search_settings(
    input: WebSearchSettingsInput,
) -> Result<BridgeStudioSnapshotResponse, BridgeError> {
    let bridge = active_bridge().await?;
    let mut config = bridge.studio.config_store().load_or_default()?;
    config.web_search = web_search_config_from_input(input)?;
    config.validate()?;
    bridge.studio.config_store().save(&config)?;
    Ok(studio_snapshot_inner(bridge, None, None).await?)
}

pub async fn save_runtime_permission_mode(
    mode: String,
) -> Result<BridgeStudioSnapshotResponse, BridgeError> {
    let bridge = active_bridge().await?;
    let mut config = bridge.studio.config_store().load_or_default()?;
    config.runtime.permission_mode = PermissionMode::from_label(&mode);
    bridge.studio.config_store().save(&config)?;
    Ok(studio_snapshot_inner(bridge, None, None).await?)
}

pub async fn save_provider_settings(
    input: ProviderSettingsInput,
) -> Result<BridgeStudioSnapshotResponse, BridgeError> {
    let bridge = active_bridge().await?;
    let current = bridge.studio.config_store().load_or_default()?;
    let next = provider_settings_edit(input, &current)?.to_config(&current)?;
    bridge.studio.config_store().save(&next)?;
    bridge.studio.reconcile_mcp_runtime().await?;
    Ok(studio_snapshot_inner(bridge, None, None).await?)
}

pub async fn save_instructions_settings(
    input: InstructionsSettingsInput,
) -> Result<BridgeStudioSnapshotResponse, BridgeError> {
    let bridge = active_bridge().await?;
    let mut config = bridge.studio.config_store().load_or_default()?;
    config.instructions.base_override = input.base_override;
    config.instructions.developer = input.developer;
    config.instructions.user = input.user;
    config.instructions.project_doc_max_bytes = input.project_doc_max_bytes;
    config.instructions.project_doc_fallback_filenames =
        normalized_string_list(input.project_doc_fallback_filenames);
    config.validate()?;
    bridge.studio.config_store().save(&config)?;
    Ok(studio_snapshot_inner(bridge, None, None).await?)
}

pub async fn save_skills_settings(
    input: SkillsSettingsInput,
) -> Result<BridgeStudioSnapshotResponse, BridgeError> {
    let bridge = active_bridge().await?;
    let mut config = bridge.studio.config_store().load_or_default()?;
    config.skills.enabled = input.enabled;
    config.skills.auto_learn = input.auto_learn;
    config.skills.system.enabled = input.system_enabled;
    config.skills.project_dir = input.project_dir;
    config.skills.user_dir = input.user_dir;
    config.skills.external_dirs = input.external_dirs;
    config.skills.disabled = input.disabled;
    config.skills.auto_learn_min_tool_calls = input.auto_learn_min_tool_calls;
    config.validate()?;
    bridge.studio.config_store().save(&config)?;
    Ok(studio_snapshot_inner(bridge, None, None).await?)
}

pub async fn save_mcp_settings(
    input: McpSettingsInput,
) -> Result<BridgeStudioSnapshotResponse, BridgeError> {
    let bridge = active_bridge().await?;
    let mut config = bridge.studio.config_store().load_or_default()?;
    let mut next_servers = std::mem::take(&mut config.mcp.servers);
    let mut next_builtin = std::mem::take(&mut config.mcp.builtin_servers);
    for server in input.servers {
        let server_id = server.id.trim().to_string();
        if server_id.is_empty() {
            continue;
        }
        if is_builtin_mcp_server_id(&server_id) {
            next_builtin.insert(
                server_id,
                BuiltinMcpServerState {
                    enabled: server.enabled,
                },
            );
            continue;
        }
        let mut mcp_config = next_servers
            .remove(&server_id)
            .unwrap_or_else(|| McpServerConfig {
                transport: mcp_transport_from_label(&server.transport),
                ..Default::default()
            });
        mcp_config.enabled = server.enabled;
        if !server.transport.trim().is_empty() {
            mcp_config.transport = mcp_transport_from_label(&server.transport);
        }
        let endpoint = server.endpoint.trim();
        match mcp_config.transport {
            McpServerTransport::Stdio => {
                mcp_config.command = (!endpoint.is_empty()).then(|| endpoint.to_string());
            }
            McpServerTransport::StreamableHttp => {
                mcp_config.url = (!endpoint.is_empty()).then(|| endpoint.to_string());
            }
        }
        next_servers.insert(server_id, mcp_config);
    }
    config.mcp.servers = next_servers;
    config.mcp.builtin_servers = next_builtin;
    config.validate()?;
    bridge.studio.config_store().save(&config)?;
    bridge.studio.reconcile_mcp_runtime().await?;
    Ok(studio_snapshot_inner(bridge, None, None).await?)
}

pub async fn save_general_settings(
    input: crate::api::studio::types::GeneralSettingsInput,
) -> Result<BridgeStudioSnapshotResponse, BridgeError> {
    let bridge = active_bridge().await?;
    let normalized = serde_json::to_string(&input)?;
    bridge
        .studio
        .store()
        .save_setting("flutterSettings:general", &normalized)
        .await?;
    Ok(studio_snapshot_inner(bridge, None, None).await?)
}
