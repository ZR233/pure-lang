//! Studio Settings 命令层：把各类设置更新请求写入配置 runtime 并发布 canonical snapshot。

use anyhow::{Context, Result, bail};
use pl_protocol::ThreadModeId;
use pl_protocol::studio::{
    SetModeModelRouteRequest, SetModelRoleRequest, SetThreadModelRouteRequest,
    StudioSettingsSnapshot, ThreadModelRouteUpdateResponse, UpdateDeepSeekWebSearchSettingsRequest,
    UpdateGeneralSettingsRequest, UpdateInstructionsSettingsRequest, UpdateMcpSettingsRequest,
    UpdatePermissionSettingsRequest, UpdateProviderSettingsRequest, UpdateSkillsSettingsRequest,
    UpdateWebSearchSettingsRequest,
};

use crate::config::{ModelRouteConfig, ProviderId, ReasoningEffort};
use crate::{ModeRouteEdit, PermissionMode, ProviderSettingsEdit, RoleEdit, StudioRole};

use super::StudioRuntime;

mod provider_edit;
mod view;

use provider_edit::{invalid_settings_argument, web_search_config};
use view::normalized_string_list;
pub(crate) use view::settings_snapshot;

impl StudioRuntime {
    /// Reads the canonical built-in provider and model catalog.
    pub fn load_provider_catalog(&self) -> Result<pl_protocol::ProviderCatalogSnapshot> {
        Ok(pl_model::config::builtin_provider_catalog().snapshot()?)
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
        anyhow::ensure!(
            input
                .sidebar_width
                .is_none_or(|width| (300..=440).contains(&width)),
            "sidebar width must be between 300 and 440"
        );
        anyhow::ensure!(
            input.pinned_thread_ids.len() <= 64 && input.pinned_project_ids.len() <= 64,
            "at most 64 pinned projects or sessions are supported"
        );
        let state = self
            .config_runtime
            .update(request.expected_revision, |config| {
                let mut config = config.clone();
                config.ui.follow_active_turn = input.follow_active_turn;
                config.ui.compact_timeline = input.compact_timeline;
                config.ui.sidebar_width = input.sidebar_width;
                config.ui.pinned_thread_ids = input.pinned_thread_ids;
                config.ui.pinned_project_ids = input.pinned_project_ids;
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
            if crate::config::mcp::is_builtin_mcp_server_id(&server_id) {
                next_builtin.insert(
                    server_id,
                    crate::config::mcp::BuiltinMcpServerState {
                        enabled: server.enabled,
                    },
                );
                continue;
            }
            let transport = match server.transport.trim() {
                "stdio" => pl_tool::mcp::config::McpServerTransport::Stdio,
                "streamableHttp" => pl_tool::mcp::config::McpServerTransport::StreamableHttp,
                _ => return Err(invalid_settings_argument("Unsupported MCP transport")),
            };
            let mut mcp_config = next_servers.remove(&server_id).unwrap_or_else(|| {
                pl_tool::mcp::config::McpServerConfig {
                    transport,
                    ..Default::default()
                }
            });
            mcp_config.enabled = server.enabled;
            mcp_config.transport = transport;
            let endpoint = server.endpoint.trim();
            match transport {
                pl_tool::mcp::config::McpServerTransport::Stdio => {
                    mcp_config.command = (!endpoint.is_empty()).then(|| endpoint.to_string());
                }
                pl_tool::mcp::config::McpServerTransport::StreamableHttp => {
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
            mode_routes: request
                .mode_routes
                .into_iter()
                .map(ModeRouteEdit::from)
                .collect(),
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
        anyhow::ensure!(
            role != StudioRole::Planner,
            "planner is a root identity, not a configurable child model role"
        );
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

    pub fn save_mode_model_route(
        &self,
        request: SetModeModelRouteRequest,
    ) -> Result<StudioSettingsSnapshot> {
        let mode = ThreadModeId::new(request.mode_id.trim().to_string())
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let state = self.set_mode_model_route(
            request.expected_revision,
            mode,
            &request.provider_id,
            &request.model,
            request.effort.as_deref(),
        )?;
        self.publish_settings_state(state.clone())?;
        settings_snapshot(state)
    }

    pub async fn save_thread_model_route(
        &self,
        thread_id: &str,
        request: SetThreadModelRouteRequest,
    ) -> Result<ThreadModelRouteUpdateResponse> {
        let _guard = self.lifecycle_lock.lock().await;
        let record = self.read_owned_thread(thread_id).await?;
        anyhow::ensure!(
            record.parent_thread_id.is_none(),
            "child Thread model routes are frozen by their Agent Profile"
        );
        let thread = self.ensure_thread_owner(thread_id).await?;
        let state = thread.snapshot();
        let settings = self.config_runtime.read()?;
        if settings.revision != request.expected_settings_revision {
            return Err(crate::ConfigRuntimeError::StaleRevision {
                expected: request.expected_settings_revision,
                actual: settings.revision,
            }
            .into());
        }
        let mode = crate::studio::thread_projection::saved_mode(&state)?
            .unwrap_or_else(|| record.mode.clone());
        let mode_definition = self
            .thread_modes
            .snapshot()
            .mode(&mode)
            .context("current Thread Mode is unavailable")?;
        let selector = validated_route(
            &settings.config,
            &format!("Thread {thread_id}"),
            &request.provider_id,
            &request.model,
            request.effort.as_deref(),
        )?;
        let route = settings
            .config
            .models
            .resolve_route(StudioRole::Planner.id(), &selector)?;
        crate::mode::validate_thread_mode_model(Some(&mode_definition), &route.model)?;
        let previous = state
            .extensions
            .get(crate::studio::model_route::MODEL_ROUTE_EXTENSION)
            .context("Thread has no saved model route")?;
        thread
            .queue_deferred_model_update_if_current(
                Self::deferred_model_update(&route, &settings.config)?,
                pl_core::thread::DeferredModelUpdatePrecondition::commit_sequence(
                    request.expected_thread_revision,
                ),
                vec![pl_core::thread::extensions::ExtensionMutation::Put {
                    id: crate::studio::model_route::MODEL_ROUTE_EXTENSION.into(),
                    expected_revision: Some(previous.revision),
                    payload: crate::studio::model_route::encode(&selector)?,
                }],
            )
            .await?;
        self.tool_catalog_updates.notify_one();

        let mut mode_default_saved = false;
        let mut warning = None;
        for _ in 0..3 {
            let latest = self.config_runtime.read()?;
            let mut config = latest.config;
            let candidate = match validated_route(
                &config,
                &format!("Thread Mode {mode}"),
                &request.provider_id,
                &request.model,
                request.effort.as_deref(),
            ) {
                Ok(candidate) => candidate,
                Err(error) => {
                    warning = Some(format!(
                        "current Thread was updated, but its Mode default was not saved: {error}"
                    ));
                    break;
                }
            };
            config.mode_model_routes.insert(mode.clone(), candidate);
            match self.config_runtime.replace(latest.revision, config) {
                Ok(updated) => {
                    if let Err(error) = self.publish_settings_state(updated) {
                        warning = Some(format!(
                            "current Thread and Mode default were saved, but the Settings event was not published: {error}"
                        ));
                    }
                    mode_default_saved = true;
                    break;
                }
                Err(crate::ConfigRuntimeError::StaleRevision { .. }) => continue,
                Err(error) => {
                    warning = Some(format!(
                        "current Thread was updated, but its Mode default was not saved: {error}"
                    ));
                    break;
                }
            }
        }
        if !mode_default_saved && warning.is_none() {
            warning = Some(
                "current Thread was updated, but its Mode default conflicted repeatedly".into(),
            );
        }
        let snapshot = self.thread_snapshot(thread_id).await?;
        let runtime = snapshot
            .runtime
            .context("Thread runtime snapshot is unavailable")?;
        Ok(ThreadModelRouteUpdateResponse {
            runtime,
            settings: self.read_settings()?,
            mode_default_saved,
            warning,
        })
    }

    pub fn set_mode_model_route(
        &self,
        expected_settings_revision: u64,
        mode: ThreadModeId,
        provider_id: &str,
        model_slug: &str,
        effort: Option<&str>,
    ) -> Result<crate::ConfigRuntimeSnapshot> {
        let current = self.config_runtime.read()?;
        anyhow::ensure!(
            current.revision == expected_settings_revision,
            "settings revision conflict: expected {expected_settings_revision}, actual {}",
            current.revision
        );
        let mut config = current.config;
        let next_route = validated_route(
            &config,
            &format!("Thread Mode {mode}"),
            provider_id,
            model_slug,
            effort,
        )?;
        config.mode_model_routes.insert(mode, next_route);
        config.validate()?;
        Ok(self.config_runtime.replace(current.revision, config)?)
    }

    pub fn set_model_role(
        &self,
        expected_settings_revision: u64,
        role: StudioRole,
        provider_id: &str,
        model_slug: &str,
        effort: Option<&str>,
    ) -> Result<crate::ConfigRuntimeSnapshot> {
        anyhow::ensure!(
            StudioRole::child_roles().contains(&role),
            "planner is a root identity, not a configurable child model role"
        );
        let current = self.config_runtime.read()?;
        anyhow::ensure!(
            current.revision == expected_settings_revision,
            "settings revision conflict: expected {expected_settings_revision}, actual {}",
            current.revision
        );
        let mut config = current.config;
        let next_route = validated_route(
            &config,
            &format!("role {}", role.key()),
            provider_id,
            model_slug,
            effort,
        )?;
        config.models.routes.insert(role.id(), next_route);
        config.validate()?;
        Ok(self.config_runtime.replace(current.revision, config)?)
    }
}

fn validated_route(
    config: &crate::StudioConfig,
    subject: &str,
    provider_id: &str,
    model_slug: &str,
    effort: Option<&str>,
) -> Result<ModelRouteConfig> {
    let provider_id = provider_id.trim();
    let model_slug = model_slug.trim();
    let provider_key = ProviderId::new(provider_id)?;
    let provider = config
        .models
        .providers
        .get(&provider_key)
        .with_context(|| format!("{subject} references missing provider: {provider_id}"))?;
    let models = provider.effective_models()?;
    let model = models
        .iter()
        .find(|model| model.slug == model_slug)
        .with_context(|| {
            format!("{subject} references missing model: {provider_id}.{model_slug}")
        })?;
    let resolved_effort = match effort.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => {
            if !model
                .supported_efforts()
                .iter()
                .any(|candidate| candidate == value)
            {
                bail!(
                    "{subject} uses unsupported effort '{value}' for model {provider_id}.{model_slug}"
                );
            }
            Some(value.to_string())
        }
        None => model.default_effort(),
    };
    Ok(ModelRouteConfig {
        provider: provider_key,
        model: model_slug.to_string(),
        effort: resolved_effort.map(ReasoningEffort::new),
    })
}
