use super::snapshot::settings_snapshot;
use crate::api::studio::bridge_runtime::active_bridge;
use crate::api::studio::convert::settings::{
    mcp_transport_from_label, normalized_string_list, provider_settings_edit,
    web_search_config_from_input, web_search_settings_dto,
};
use crate::api::studio::types::{
    BridgeError, BridgeProviderCatalogSnapshot, BridgeSettingsStateSnapshot,
    BridgeWebSearchSettingsDto, InstructionsSettingsInput, McpSettingsInput, ProviderSettingsInput,
    SkillsSettingsInput, WebSearchSettingsInput,
};
use anyhow::Context;
use pl_studio_runtime::{
    BuiltinMcpServerState, McpServerConfig, McpServerTransport, PermissionMode, StudioRole,
    is_builtin_mcp_server_id,
};
// ── Settings ──

pub fn load_provider_catalog() -> Result<BridgeProviderCatalogSnapshot, BridgeError> {
    Ok(pl_studio_runtime::builtin_provider_catalog()
        .snapshot()?
        .into())
}

pub async fn read_web_search_settings() -> Result<BridgeWebSearchSettingsDto, BridgeError> {
    let bridge = active_bridge().await?;
    let config = bridge.studio.settings_state()?.config;
    Ok(web_search_settings_dto(
        &config,
        pl_studio_runtime::StudioRole::Executor,
    )?)
}

pub async fn save_web_search_settings(
    expected_settings_revision: u64,
    input: WebSearchSettingsInput,
) -> Result<BridgeSettingsStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    let web_search = web_search_config_from_input(input)?;
    let state = bridge
        .studio
        .config_runtime()
        .update(expected_settings_revision, |config| {
            let mut config = config.clone();
            config.web_search = web_search;
            Ok(config)
        })?;
    bridge.studio.publish_settings_state(state.clone());
    Ok(settings_snapshot(&state)?)
}

pub async fn read_settings_state() -> Result<BridgeSettingsStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(settings_snapshot(&bridge.studio.settings_state()?)?)
}

pub async fn reload_settings_from_disk(
    expected_settings_revision: u64,
) -> Result<BridgeSettingsStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    let state = bridge
        .studio
        .config_runtime()
        .reload_from_disk(expected_settings_revision)?;
    bridge.studio.publish_settings_state(state.clone());
    bridge.studio.skill_catalog_runtime().mark_all_stale().await;
    let _ = bridge.studio.apply_provider_config(&state.config).await?;
    bridge.studio.reconcile_mcp_runtime().await?;
    Ok(settings_snapshot(&state)?)
}

pub async fn save_runtime_permission_mode(
    expected_settings_revision: u64,
    mode: String,
) -> Result<BridgeSettingsStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    let permission_mode = PermissionMode::from_label(&mode).ok_or_else(|| {
        pl_studio_runtime::PureError::ConfigError(format!("unsupported permission mode: {mode}"))
    })?;
    let state = bridge
        .studio
        .config_runtime()
        .update(expected_settings_revision, |config| {
            let mut config = config.clone();
            config.runtime.permission_mode = permission_mode;
            Ok(config)
        })?;
    bridge.studio.publish_settings_state(state.clone());
    Ok(settings_snapshot(&state)?)
}

pub async fn save_provider_settings(
    expected_settings_revision: u64,
    input: ProviderSettingsInput,
) -> Result<BridgeSettingsStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    let current = bridge.studio.settings_state()?;
    let next = provider_settings_edit(input, &current.config)?.to_config(&current.config)?;
    let state = bridge
        .studio
        .config_runtime()
        .replace(expected_settings_revision, next)?;
    bridge.studio.publish_settings_state(state.clone());
    let _ = bridge.studio.apply_provider_config(&state.config).await?;
    bridge.studio.reconcile_mcp_runtime().await?;
    Ok(settings_snapshot(&state)?)
}

pub async fn save_instructions_settings(
    expected_settings_revision: u64,
    input: InstructionsSettingsInput,
) -> Result<BridgeSettingsStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    let state = bridge
        .studio
        .config_runtime()
        .update(expected_settings_revision, |config| {
            let mut config = config.clone();
            config.instructions.base_override = input.base_override;
            config.instructions.developer = input.developer;
            config.instructions.user = input.user;
            config.instructions.project_doc_max_bytes = input.project_doc_max_bytes;
            config.instructions.project_doc_fallback_filenames =
                normalized_string_list(input.project_doc_fallback_filenames);
            Ok(config)
        })?;
    bridge.studio.publish_settings_state(state.clone());
    Ok(settings_snapshot(&state)?)
}

pub async fn save_skills_settings(
    expected_settings_revision: u64,
    input: SkillsSettingsInput,
) -> Result<BridgeSettingsStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    let state = bridge
        .studio
        .config_runtime()
        .update(expected_settings_revision, |config| {
            let mut config = config.clone();
            config.skills.enabled = input.enabled;
            config.skills.auto_learn = input.auto_learn;
            config.skills.system.enabled = input.system_enabled;
            config.skills.project_dir = input.project_dir;
            config.skills.user_dir = input.user_dir;
            config.skills.external_dirs = input.external_dirs;
            config.skills.disabled = input.disabled;
            config.skills.auto_learn_min_tool_calls = input.auto_learn_min_tool_calls;
            Ok(config)
        })?;
    bridge.studio.publish_settings_state(state.clone());
    bridge.studio.skill_catalog_runtime().mark_all_stale().await;
    Ok(settings_snapshot(&state)?)
}

pub async fn save_mcp_settings(
    expected_settings_revision: u64,
    input: McpSettingsInput,
) -> Result<BridgeSettingsStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    let mut config = bridge.studio.settings_state()?.config;
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
        let transport = mcp_transport_from_label(&server.transport)?;
        let mut mcp_config = next_servers
            .remove(&server_id)
            .unwrap_or_else(|| McpServerConfig {
                transport,
                ..Default::default()
            });
        mcp_config.enabled = server.enabled;
        mcp_config.transport = transport;
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
    let state = bridge
        .studio
        .config_runtime()
        .replace(expected_settings_revision, config)?;
    bridge.studio.publish_settings_state(state.clone());
    bridge.studio.reconcile_mcp_runtime().await?;
    Ok(settings_snapshot(&state)?)
}

pub async fn save_general_settings(
    expected_settings_revision: u64,
    input: crate::api::studio::types::GeneralSettingsInput,
) -> Result<BridgeSettingsStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    let state = bridge
        .studio
        .config_runtime()
        .update(expected_settings_revision, |config| {
            let mut config = config.clone();
            config.ui.follow_system_theme = input.follow_system_theme;
            config.ui.follow_active_turn = input.follow_active_turn;
            config.ui.compact_timeline = input.compact_timeline;
            Ok(config)
        })?;
    bridge.studio.publish_settings_state(state.clone());
    Ok(settings_snapshot(&state)?)
}

pub async fn set_model_role(
    expected_settings_revision: u64,
    role_key: String,
    provider_id: String,
    model: String,
    effort: Option<String>,
) -> Result<BridgeSettingsStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    let role = StudioRole::from_key(role_key.trim())
        .with_context(|| format!("unsupported model role: {role_key}"))?;
    let state = bridge.studio.set_model_role(
        expected_settings_revision,
        role,
        &provider_id,
        &model,
        effort.as_deref(),
    )?;
    bridge.studio.publish_settings_state(state.clone());
    Ok(settings_snapshot(&state)?)
}
