//! Studio Settings 命令层：把各类设置更新请求写入配置 runtime 并发布 canonical snapshot。

use anyhow::{Context, Result, bail};
use pl_protocol::studio::{
    SetModelRoleRequest, StudioSettingsSnapshot, UpdateDeepSeekWebSearchSettingsRequest,
    UpdateGeneralSettingsRequest, UpdateInstructionsSettingsRequest, UpdateMcpSettingsRequest,
    UpdatePermissionSettingsRequest, UpdateProviderSettingsRequest, UpdateSkillsSettingsRequest,
    UpdateWebSearchSettingsRequest,
};

use crate::config::{ModelRouteConfig, ProviderId, ReasoningEffort};
use crate::{PermissionMode, ProviderSettingsEdit, RoleEdit, StudioRole};

use super::StudioRuntime;

mod provider_edit;
mod view;

use provider_edit::{invalid_settings_argument, web_search_config};
use view::normalized_string_list;
pub(crate) use view::settings_snapshot;

impl StudioRuntime {
    /// Reads the canonical built-in provider and model catalog.
    pub fn load_provider_catalog(&self) -> Result<pl_protocol::ProviderCatalogSnapshot> {
        Ok(crate::builtin_provider_catalog().snapshot()?)
    }

    /// Reads the secret-free canonical Settings snapshot from the in-memory owner.
    pub fn read_settings(&self) -> Result<StudioSettingsSnapshot> {
        settings_snapshot(self.config_runtime.read()?)
    }

    pub fn save_permission_settings(
        &self,
        request: UpdatePermissionSettingsRequest,
    ) -> Result<StudioSettingsSnapshot> {
        let mode = PermissionMode::from_label(&request.mode)
            .ok_or_else(|| invalid_settings_argument("Unsupported permission mode"))?;
        let state = self
            .config_runtime
            .update(request.expected_revision, |config| {
                let mut config = config.clone();
                config.runtime.permission_mode = mode;
                Ok(config)
            })?;
        self.publish_settings_state(state.clone())?;
        settings_snapshot(state)
    }

    pub fn save_instructions_settings(
        &self,
        request: UpdateInstructionsSettingsRequest,
    ) -> Result<StudioSettingsSnapshot> {
        let input = request.settings;
        let state = self
            .config_runtime
            .update(request.expected_revision, |config| {
                let mut config = config.clone();
                config.instructions.base_override = input.base_override;
                config.instructions.developer = input.developer;
                config.instructions.user = input.user;
                config.instructions.project_doc_max_bytes =
                    usize::try_from(input.project_doc_max_bytes).map_err(|_| {
                        pl_protocol::PureError::ConfigError(
                            "projectDocMaxBytes exceeds this platform".to_string(),
                        )
                    })?;
                config.instructions.project_doc_fallback_filenames =
                    normalized_string_list(input.project_doc_fallback_filenames);
                Ok(config)
            })?;
        self.publish_settings_state(state.clone())?;
        settings_snapshot(state)
    }

    pub async fn save_skills_settings(
        &self,
        request: UpdateSkillsSettingsRequest,
    ) -> Result<StudioSettingsSnapshot> {
        let input = request.settings;
        let state = self
            .config_runtime
            .update(request.expected_revision, |config| {
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
        self.publish_settings_state(state.clone())?;
        self.skills.mark_all_stale().await;
        settings_snapshot(state)
    }

    pub fn save_general_settings(
        &self,
        request: UpdateGeneralSettingsRequest,
    ) -> Result<StudioSettingsSnapshot> {
        let input = request.settings;
        let state = self
            .config_runtime
            .update(request.expected_revision, |config| {
                let mut config = config.clone();
                config.ui.follow_system_theme = input.follow_system_theme;
                config.ui.follow_active_turn = input.follow_active_turn;
                config.ui.compact_timeline = input.compact_timeline;
                Ok(config)
            })?;
        self.publish_settings_state(state.clone())?;
        settings_snapshot(state)
    }

    pub fn save_web_search_settings(
        &self,
        request: UpdateWebSearchSettingsRequest,
    ) -> Result<StudioSettingsSnapshot> {
        let web_search = web_search_config(request)?;
        let expected_revision = web_search.0;
        let state = self.config_runtime.update(expected_revision, |config| {
            let mut config = config.clone();
            config.web_search = web_search.1;
            Ok(config)
        })?;
        self.publish_settings_state(state.clone())?;
        settings_snapshot(state)
    }

    pub fn save_deepseek_web_search_settings(
        &self,
        request: UpdateDeepSeekWebSearchSettingsRequest,
    ) -> Result<StudioSettingsSnapshot> {
        let state = self
            .config_runtime
            .update(request.expected_revision, |config| {
                let mut config = config.clone();
                config.deepseek_web_search.enabled = request.enabled;
                Ok(config)
            })?;
        self.publish_settings_state(state.clone())?;
        settings_snapshot(state)
    }

    pub async fn reload_settings(&self, expected_revision: u64) -> Result<StudioSettingsSnapshot> {
        let state = self.config_runtime.reload_from_disk(expected_revision)?;
        self.publish_settings_state(state.clone())?;
        self.skills.mark_all_stale().await;
        let _ = self.apply_provider_config(&state.config).await?;
        self.reconcile_mcp_runtime().await?;
        settings_snapshot(state)
    }

    pub async fn save_mcp_settings(
        &self,
        request: UpdateMcpSettingsRequest,
    ) -> Result<StudioSettingsSnapshot> {
        let mut config = self.config_runtime.read()?.config;
        let mut next_servers = std::mem::take(&mut config.mcp.servers);
        let mut next_builtin = std::mem::take(&mut config.mcp.builtin_servers);
        for server in request.servers {
            let server_id = server.id.trim().to_string();
            if server_id.is_empty() {
                continue;
            }
            if crate::is_builtin_mcp_server_id(&server_id) {
                next_builtin.insert(
                    server_id,
                    crate::BuiltinMcpServerState {
                        enabled: server.enabled,
                    },
                );
                continue;
            }
            let transport = match server.transport.trim() {
                "stdio" => crate::McpServerTransport::Stdio,
                "streamableHttp" => crate::McpServerTransport::StreamableHttp,
                _ => return Err(invalid_settings_argument("Unsupported MCP transport")),
            };
            let mut mcp_config =
                next_servers
                    .remove(&server_id)
                    .unwrap_or_else(|| crate::McpServerConfig {
                        transport,
                        ..Default::default()
                    });
            mcp_config.enabled = server.enabled;
            mcp_config.transport = transport;
            let endpoint = server.endpoint.trim();
            match transport {
                crate::McpServerTransport::Stdio => {
                    mcp_config.command = (!endpoint.is_empty()).then(|| endpoint.to_string());
                }
                crate::McpServerTransport::StreamableHttp => {
                    mcp_config.url = (!endpoint.is_empty()).then(|| endpoint.to_string());
                }
            }
            next_servers.insert(server_id, mcp_config);
        }
        config.mcp.servers = next_servers;
        config.mcp.builtin_servers = next_builtin;
        let state = self
            .config_runtime
            .replace(request.expected_revision, config)?;
        self.publish_settings_state(state.clone())?;
        self.reconcile_mcp_runtime().await?;
        settings_snapshot(state)
    }

    pub async fn save_provider_settings(
        &self,
        request: UpdateProviderSettingsRequest,
    ) -> Result<StudioSettingsSnapshot> {
        let current = self.config_runtime.read()?;
        let edit = ProviderSettingsEdit {
            default_provider: Some(request.default_provider_id),
            providers: request
                .providers
                .into_iter()
                .map(|provider| provider_edit::provider_edit(provider, &current.config))
                .collect::<Result<Vec<_>>>()?,
            roles: request.roles.into_iter().map(RoleEdit::from).collect(),
        };
        let next = edit.to_config(&current.config)?;
        let state = self
            .config_runtime
            .replace(request.expected_revision, next)?;
        self.publish_settings_state(state.clone())?;
        let _ = self.apply_provider_config(&state.config).await?;
        self.reconcile_mcp_runtime().await?;
        settings_snapshot(state)
    }

    pub fn save_model_role(&self, request: SetModelRoleRequest) -> Result<StudioSettingsSnapshot> {
        let role = StudioRole::from_key(request.role.trim())
            .ok_or_else(|| invalid_settings_argument("Unsupported model role"))?;
        let state = self.set_model_role(
            request.expected_revision,
            role,
            &request.provider_id,
            &request.model,
            request.effort.as_deref(),
        )?;
        self.publish_settings_state(state.clone())?;
        settings_snapshot(state)
    }

    pub fn set_model_role(
        &self,
        expected_settings_revision: u64,
        role: StudioRole,
        provider_id: &str,
        model_slug: &str,
        effort: Option<&str>,
    ) -> Result<crate::ConfigRuntimeSnapshot> {
        let provider_id = provider_id.trim();
        let model_slug = model_slug.trim();
        let current = self.config_runtime.read()?;
        anyhow::ensure!(
            current.revision == expected_settings_revision,
            "settings revision conflict: expected {expected_settings_revision}, actual {}",
            current.revision
        );
        let mut config = current.config;
        let provider_key = ProviderId::new(provider_id)?;
        let resolved_effort = {
            let provider = config
                .models
                .providers
                .get(&provider_key)
                .with_context(|| {
                    format!(
                        "role {} references missing provider: {provider_id}",
                        role.key()
                    )
                })?;
            let models = provider.effective_models()?;
            let model = models
                .iter()
                .find(|model| model.slug == model_slug)
                .with_context(|| {
                    format!(
                        "role {} references missing model: {provider_id}.{model_slug}",
                        role.key()
                    )
                })?;
            match effort.map(str::trim).filter(|value| !value.is_empty()) {
                Some(value) => {
                    if !model
                        .supported_efforts()
                        .iter()
                        .any(|candidate| candidate == value)
                    {
                        bail!(
                            "role {} uses unsupported effort '{}' for model {provider_id}.{model_slug}",
                            role.key(),
                            value
                        );
                    }
                    Some(value.to_string())
                }
                None => model.default_effort(),
            }
        };
        let next_route = ModelRouteConfig {
            provider: provider_key,
            model: model_slug.to_string(),
            effort: resolved_effort.map(ReasoningEffort::new),
        };
        config.models.routes.insert(role.id(), next_route);
        config.validate()?;
        Ok(self.config_runtime.replace(current.revision, config)?)
    }
}
