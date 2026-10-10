//! Studio Settings 命令层：把各类设置更新请求写入配置 runtime 并发布 canonical snapshot。

use anyhow::{Context, Result, bail};
use pl_protocol::ThreadModeId;
use pl_protocol::studio::{
    RemoveProviderRequest, SetDefaultProviderRequest, SetThreadModelRouteRequest,
    SettingsFieldUpdate, SettingsStateResponse, ThreadModelRouteUpdateResponse,
    UpdatePermissionSettingsRequest, UpdateProviderRequest, UpdateSettingsFieldRequest,
};

use crate::config::{ModelRouteConfig, ProviderId, ReasoningEffort};
use crate::{PermissionMode, StudioRole};

use super::StudioRuntime;

mod provider_edit;
mod view;

use provider_edit::invalid_settings_argument;
pub(crate) use view::{model_catalog_snapshot, settings_config_snapshot, settings_state_response};
use view::{normalized_optional, normalized_string_list};

impl StudioRuntime {
    fn settings_response(
        &self,
        state: &crate::ConfigRuntimeSnapshot,
    ) -> Result<SettingsStateResponse> {
        settings_state_response(state, &self.config_runtime.read_catalog()?)
    }

    /// Reads the canonical built-in provider and model catalog.
    pub fn load_provider_catalog(&self) -> Result<pl_protocol::ProviderCatalogSnapshot> {
        Ok(pl_model::config::builtin_provider_catalog()?.snapshot()?)
    }

    /// Reads the secret-free canonical Settings snapshot from the in-memory owner.
    pub fn read_settings(&self) -> Result<SettingsStateResponse> {
        self.settings_response(&self.config_runtime.read()?)
    }

    /// Applies one typed settings field/resource mutation.
    ///
    /// The closure starts from the runtime's canonical desired configuration,
    /// so a command never carries an old sibling field back into the store.
    /// Route model and effort updates deliberately have separate variants; the
    /// unchanged half is resolved from the current canonical route.
    pub async fn apply_settings_field(
        &self,
        request: UpdateSettingsFieldRequest,
    ) -> Result<SettingsStateResponse> {
        let mark_skills_stale = matches!(
            &request.update,
            SettingsFieldUpdate::SkillsEnabled { .. }
                | SettingsFieldUpdate::SkillsAutoLearn { .. }
                | SettingsFieldUpdate::SkillsSystemEnabled { .. }
                | SettingsFieldUpdate::SkillsProjectDir { .. }
                | SettingsFieldUpdate::SkillsUserDir { .. }
                | SettingsFieldUpdate::SkillsExternalDirs { .. }
                | SettingsFieldUpdate::SkillsDisabled { .. }
                | SettingsFieldUpdate::SkillsAutoLearnMinToolCalls { .. }
        );
        let reconcile_mcp = matches!(
            &request.update,
            SettingsFieldUpdate::McpServerEnabled { .. }
                | SettingsFieldUpdate::McpServerTransport { .. }
                | SettingsFieldUpdate::McpServerEndpoint { .. }
        );
        let update = request.update;
        let state = self.config_runtime.update_with_effective(
            request.expected_revision,
            |config, effective| {
                let mut config = config.clone();
                let route_validation_config = if matches!(
                    &update,
                    SettingsFieldUpdate::ModeModel { .. }
                        | SettingsFieldUpdate::ModeReasoningEffort { .. }
                        | SettingsFieldUpdate::RoleModel { .. }
                        | SettingsFieldUpdate::RoleReasoningEffort { .. }
                ) {
                    effective
                } else {
                    &config
                };
                match update {
                    SettingsFieldUpdate::InstructionBaseOverride { value } => {
                        config.instructions.base_override = value;
                    }
                    SettingsFieldUpdate::InstructionDeveloper { value } => {
                        config.instructions.developer = value;
                    }
                    SettingsFieldUpdate::InstructionUser { value } => {
                        config.instructions.user = value;
                    }
                    SettingsFieldUpdate::ProjectDocMaxBytes { value } => {
                        config.instructions.project_doc_max_bytes = usize::try_from(value)
                            .map_err(|_| {
                                pl_protocol::PureError::ConfigError(
                                    "projectDocMaxBytes exceeds this platform".to_string(),
                                )
                            })?;
                    }
                    SettingsFieldUpdate::ProjectDocFallbackFilenames { value } => {
                        config.instructions.project_doc_fallback_filenames =
                            normalized_string_list(value);
                    }
                    SettingsFieldUpdate::SkillsEnabled { value } => config.skills.enabled = value,
                    SettingsFieldUpdate::SkillsAutoLearn { value } => {
                        config.skills.auto_learn = value;
                    }
                    SettingsFieldUpdate::SkillsSystemEnabled { value } => {
                        config.skills.system.enabled = value;
                    }
                    SettingsFieldUpdate::SkillsProjectDir { value } => {
                        config.skills.project_dir = value
                    }
                    SettingsFieldUpdate::SkillsUserDir { value } => config.skills.user_dir = value,
                    SettingsFieldUpdate::SkillsExternalDirs { value } => {
                        config.skills.external_dirs = normalized_string_list(value);
                    }
                    SettingsFieldUpdate::SkillsDisabled { value } => {
                        config.skills.disabled = normalized_string_list(value);
                    }
                    SettingsFieldUpdate::SkillsAutoLearnMinToolCalls { value } => {
                        config.skills.auto_learn_min_tool_calls = value;
                    }
                    SettingsFieldUpdate::McpServerEnabled { id, value } => {
                        apply_mcp_server_enabled(&mut config, &id, value)?;
                    }
                    SettingsFieldUpdate::McpServerTransport { id, transport } => {
                        apply_mcp_server_transport(&mut config, &id, &transport)?;
                    }
                    SettingsFieldUpdate::McpServerEndpoint { id, endpoint } => {
                        apply_mcp_server_endpoint(&mut config, &id, &endpoint)?;
                    }
                    SettingsFieldUpdate::GeneralFollowActiveTurn { value } => {
                        config.ui.follow_active_turn = value;
                    }
                    SettingsFieldUpdate::GeneralCompactTimeline { value } => {
                        config.ui.compact_timeline = value;
                    }
                    SettingsFieldUpdate::GeneralSidebarWidth { value } => {
                        if !value.is_none_or(|width| (300..=440).contains(&width)) {
                            return Err(pl_protocol::PureError::ConfigError(
                                "sidebar width must be between 300 and 440".to_string(),
                            ));
                        }
                        config.ui.sidebar_width = value;
                    }
                    SettingsFieldUpdate::GeneralPinnedThreadIds { value } => {
                        if value.len() > 64 {
                            return Err(pl_protocol::PureError::ConfigError(
                                "at most 64 pinned sessions are supported".to_string(),
                            ));
                        }
                        config.ui.pinned_thread_ids = normalized_string_list(value);
                    }
                    SettingsFieldUpdate::GeneralPinnedProjectIds { value } => {
                        if value.len() > 64 {
                            return Err(pl_protocol::PureError::ConfigError(
                                "at most 64 pinned projects are supported".to_string(),
                            ));
                        }
                        config.ui.pinned_project_ids = normalized_string_list(value);
                    }
                    SettingsFieldUpdate::WebSearchMode { value } => {
                        config.web_search.mode = parse_web_search_mode(&value)?;
                    }
                    SettingsFieldUpdate::WebSearchContextSize { value } => {
                        config.web_search.context_size =
                            parse_web_search_context_size(value.as_deref())?;
                    }
                    SettingsFieldUpdate::WebSearchAllowedDomains { value } => {
                        config.web_search.allowed_domains = normalized_string_list(value);
                    }
                    SettingsFieldUpdate::WebSearchCountry { value } => {
                        update_web_search_location(&mut config, |location| {
                            location.country = normalized_optional(value);
                        });
                    }
                    SettingsFieldUpdate::WebSearchRegion { value } => {
                        update_web_search_location(&mut config, |location| {
                            location.region = normalized_optional(value);
                        });
                    }
                    SettingsFieldUpdate::WebSearchCity { value } => {
                        update_web_search_location(&mut config, |location| {
                            location.city = normalized_optional(value);
                        });
                    }
                    SettingsFieldUpdate::WebSearchTimezone { value } => {
                        update_web_search_location(&mut config, |location| {
                            location.timezone = normalized_optional(value);
                        });
                    }
                    SettingsFieldUpdate::DeepSeekWebSearchEnabled { value } => {
                        config.deepseek_web_search.enabled = value;
                    }
                    SettingsFieldUpdate::ModeModel {
                        mode_id,
                        provider_id,
                        model,
                    } => {
                        let mode =
                            ThreadModeId::new(mode_id.trim().to_string()).map_err(|error| {
                                pl_protocol::PureError::ConfigError(error.to_string())
                            })?;
                        let effort = config
                            .mode_model_routes
                            .get(&mode)
                            .filter(|route| {
                                route.provider.as_str() == provider_id.trim()
                                    && route.model == model.trim()
                            })
                            .and_then(|route| route.effort.as_ref().map(|value| value.as_str()));
                        let route = validated_route(
                            route_validation_config,
                            &format!("Thread Mode {mode}"),
                            &provider_id,
                            &model,
                            effort,
                        )
                        .map_err(|error| pl_protocol::PureError::ConfigError(error.to_string()))?;
                        config.mode_model_routes.insert(mode, route);
                    }
                    SettingsFieldUpdate::ModeReasoningEffort { mode_id, effort } => {
                        let mode =
                            ThreadModeId::new(mode_id.trim().to_string()).map_err(|error| {
                                pl_protocol::PureError::ConfigError(error.to_string())
                            })?;
                        let current =
                            config
                                .mode_model_routes
                                .get(&mode)
                                .cloned()
                                .ok_or_else(|| {
                                    pl_protocol::PureError::ConfigError(format!(
                                        "Thread Mode {mode} has no model route"
                                    ))
                                })?;
                        let route = validated_route(
                            route_validation_config,
                            &format!("Thread Mode {mode}"),
                            current.provider.as_str(),
                            &current.model,
                            effort.as_deref(),
                        )
                        .map_err(|error| pl_protocol::PureError::ConfigError(error.to_string()))?;
                        config.mode_model_routes.insert(mode, route);
                    }
                    SettingsFieldUpdate::RoleModel {
                        role,
                        provider_id,
                        model,
                    } => {
                        let role = StudioRole::from_key(role.trim()).ok_or_else(|| {
                            pl_protocol::PureError::ConfigError(
                                "Unsupported model role".to_string(),
                            )
                        })?;
                        if role == StudioRole::Planner {
                            return Err(pl_protocol::PureError::ConfigError(
                                "planner is a root identity, not a configurable child model role"
                                    .to_string(),
                            ));
                        }
                        let effort = config
                            .models
                            .routes
                            .get(&role.id())
                            .filter(|route| {
                                route.provider.as_str() == provider_id.trim()
                                    && route.model == model.trim()
                            })
                            .and_then(|route| route.effort.as_ref().map(|value| value.as_str()));
                        let route = validated_route(
                            route_validation_config,
                            &format!("role {}", role.key()),
                            &provider_id,
                            &model,
                            effort,
                        )
                        .map_err(|error| pl_protocol::PureError::ConfigError(error.to_string()))?;
                        config.models.routes.insert(role.id(), route);
                    }
                    SettingsFieldUpdate::RoleReasoningEffort { role, effort } => {
                        let role = StudioRole::from_key(role.trim()).ok_or_else(|| {
                            pl_protocol::PureError::ConfigError(
                                "Unsupported model role".to_string(),
                            )
                        })?;
                        if role == StudioRole::Planner {
                            return Err(pl_protocol::PureError::ConfigError(
                                "planner is a root identity, not a configurable child model role"
                                    .to_string(),
                            ));
                        }
                        let current =
                            config
                                .models
                                .routes
                                .get(&role.id())
                                .cloned()
                                .ok_or_else(|| {
                                    pl_protocol::PureError::ConfigError(format!(
                                        "role {} has no model route",
                                        role.key()
                                    ))
                                })?;
                        let route = validated_route(
                            route_validation_config,
                            &format!("role {}", role.key()),
                            current.provider.as_str(),
                            &current.model,
                            effort.as_deref(),
                        )
                        .map_err(|error| pl_protocol::PureError::ConfigError(error.to_string()))?;
                        config.models.routes.insert(role.id(), route);
                    }
                }
                config.validate_declarations()?;
                Ok(config)
            },
        )?;
        self.publish_settings_state(state.clone())?;
        if mark_skills_stale {
            self.skills.mark_all_stale().await;
        }
        if reconcile_mcp {
            self.reconcile_mcp_runtime().await?;
        }
        self.settings_response(&state)
    }

    pub fn save_permission_settings(
        &self,
        request: UpdatePermissionSettingsRequest,
    ) -> Result<SettingsStateResponse> {
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
        self.settings_response(&state)
    }

    pub async fn reload_settings(&self, expected_revision: u64) -> Result<SettingsStateResponse> {
        let state = self.config_runtime.reload_from_disk(expected_revision)?;
        self.publish_settings_state(state.clone())?;
        self.skills.mark_all_stale().await;
        let _ = self.apply_provider_config(&state.config).await?;
        self.reconcile_mcp_runtime().await?;
        self.settings_response(&state)
    }

    pub async fn save_provider(
        &self,
        request: UpdateProviderRequest,
    ) -> Result<SettingsStateResponse> {
        let current = self.config_runtime.read()?;
        let provider = provider_edit::provider_edit(request.provider, &current.config)?;
        let next = provider.to_single_config(&current.config)?;
        let state = self
            .config_runtime
            .replace(request.expected_revision, next)?;
        self.publish_settings_state(state.clone())?;
        let _ = self.apply_provider_config(&state.config).await?;
        self.reconcile_mcp_runtime().await?;
        self.settings_response(&state)
    }

    pub fn set_default_provider(
        &self,
        request: SetDefaultProviderRequest,
    ) -> Result<SettingsStateResponse> {
        let current = self.config_runtime.read()?;
        let provider_id = ProviderId::new(request.provider_id.trim())?;
        let provider = current
            .config
            .models
            .providers
            .get(&provider_id)
            .ok_or_else(|| invalid_settings_argument("Default provider does not exist"))?;
        let models = provider.effective_models()?;
        let current_route = current
            .config
            .mode_model_routes
            .get(&ThreadModeId::simple())
            .filter(|route| route.provider == provider_id);
        let model = current_route
            .and_then(|route| models.iter().find(|model| model.slug == route.model))
            .or_else(|| {
                current
                    .config
                    .models
                    .routes
                    .values()
                    .find(|route| route.provider == provider_id)
                    .and_then(|route| models.iter().find(|model| model.slug == route.model))
            })
            .or_else(|| models.first())
            .ok_or_else(|| invalid_settings_argument("Default provider has no usable models"))?;
        let effort = current_route
            .and_then(|route| (route.model == model.slug).then_some(route.effort.as_ref()))
            .flatten()
            .map(|effort| effort.as_str().to_string())
            .or_else(|| model.default_effort());
        let route = validated_route(
            &current.config,
            "Default provider",
            provider_id.as_str(),
            &model.slug,
            effort.as_deref(),
        )?;
        let mut next = current.config.clone();
        next.mode_model_routes.insert(ThreadModeId::simple(), route);
        let state = self
            .config_runtime
            .replace(request.expected_revision, next)?;
        self.publish_settings_state(state.clone())?;
        self.settings_response(&state)
    }

    pub fn remove_provider(&self, request: RemoveProviderRequest) -> Result<SettingsStateResponse> {
        let current = self.config_runtime.read()?;
        let next = crate::config_editor::remove_provider(
            &current.config,
            &request.provider_id,
            request.replacement_provider_id.as_deref(),
        )?;
        let state = self
            .config_runtime
            .replace(request.expected_revision, next)?;
        self.publish_settings_state(state.clone())?;
        self.settings_response(&state)
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
        let route_extension_id = crate::studio::model_route::MODEL_ROUTE_EXTENSION;
        let previous = state
            .extensions
            .get(route_extension_id)
            .context("Thread has no saved model route")?;
        if previous.revision != request.expected_model_route_revision {
            return Err(crate::ConfigRuntimeError::StaleRevision {
                expected: request.expected_model_route_revision,
                actual: previous.revision,
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
        Self::validate_model_media(&state, &route.model, &[])?;
        pl_model::runtime::ThreadModel::new(
            pl_model::runtime::ModelRuntime::from_route(&route)?,
            route.reasoning_config(),
        )
        .validate_context_origin(&state.context)?;
        match thread
            .queue_deferred_model_update_if_current(
                self.deferred_model_update(&route, &settings.config)?,
                pl_core::thread::DeferredModelUpdatePrecondition::commit_sequence(
                    state.commit_sequence,
                ),
                vec![pl_core::thread::extensions::ExtensionMutation::Put {
                    id: route_extension_id.into(),
                    expected_revision: Some(request.expected_model_route_revision),
                    payload: crate::studio::model_route::encode(&selector)?,
                }],
            )
            .await
        {
            Ok(_) => {}
            Err(pl_core::thread::ThreadError::ExtensionConflict {
                id,
                expected: Some(expected),
                actual: Some(actual),
            }) if id == route_extension_id => {
                return Err(crate::ConfigRuntimeError::StaleRevision { expected, actual }.into());
            }
            Err(error) => return Err(error.into()),
        }
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
}

fn parse_web_search_mode(value: &str) -> pl_protocol::Result<pl_protocol::search::WebSearchMode> {
    match value.trim() {
        "disabled" => Ok(pl_protocol::search::WebSearchMode::Disabled),
        "cached" => Ok(pl_protocol::search::WebSearchMode::Cached),
        "indexed" => Ok(pl_protocol::search::WebSearchMode::Indexed),
        "live" => Ok(pl_protocol::search::WebSearchMode::Live),
        _ => Err(pl_protocol::PureError::ConfigError(
            "Unsupported web search mode".to_string(),
        )),
    }
}

fn parse_web_search_context_size(
    value: Option<&str>,
) -> pl_protocol::Result<Option<pl_protocol::WebSearchContextSize>> {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        None => Ok(None),
        Some("low") => Ok(Some(pl_protocol::WebSearchContextSize::Low)),
        Some("medium") => Ok(Some(pl_protocol::WebSearchContextSize::Medium)),
        Some("high") => Ok(Some(pl_protocol::WebSearchContextSize::High)),
        Some(_) => Err(pl_protocol::PureError::ConfigError(
            "Unsupported web search context size".to_string(),
        )),
    }
}

fn update_web_search_location(
    config: &mut crate::StudioConfig,
    update: impl FnOnce(&mut pl_protocol::search::WebSearchLocation),
) {
    let location = config
        .web_search
        .location
        .get_or_insert_with(Default::default);
    update(location);
    if location.is_empty() {
        config.web_search.location = None;
    }
}

fn mcp_server_id(id: &str) -> Option<String> {
    let id = id.trim().to_string();
    if id.is_empty() { None } else { Some(id) }
}

fn apply_mcp_server_enabled(
    config: &mut crate::StudioConfig,
    id: &str,
    enabled: bool,
) -> pl_protocol::Result<()> {
    let Some(id) = mcp_server_id(id) else {
        return Ok(());
    };
    if crate::config::mcp::is_builtin_mcp_server_id(&id) {
        config.mcp.builtin_servers.entry(id).or_default().enabled = enabled;
        return Ok(());
    }
    config.mcp.servers.entry(id).or_default().enabled = enabled;
    Ok(())
}

fn apply_mcp_server_transport(
    config: &mut crate::StudioConfig,
    id: &str,
    transport: &str,
) -> pl_protocol::Result<()> {
    let Some(id) = mcp_server_id(id) else {
        return Ok(());
    };
    if crate::config::mcp::is_builtin_mcp_server_id(&id) {
        return Ok(());
    }
    let transport = match transport.trim() {
        "stdio" => pl_tool::mcp::config::McpServerTransport::Stdio,
        "streamableHttp" => pl_tool::mcp::config::McpServerTransport::StreamableHttp,
        _ => {
            return Err(pl_protocol::PureError::ConfigError(
                "Unsupported MCP transport".to_string(),
            ));
        }
    };
    let server = config.mcp.servers.entry(id).or_default();
    let endpoint = match server.transport {
        pl_tool::mcp::config::McpServerTransport::Stdio => server.command.take(),
        pl_tool::mcp::config::McpServerTransport::StreamableHttp => server.url.take(),
    };
    server.transport = transport;
    match transport {
        pl_tool::mcp::config::McpServerTransport::Stdio => {
            server.command = endpoint;
            server.url = None;
        }
        pl_tool::mcp::config::McpServerTransport::StreamableHttp => {
            server.url = endpoint;
            server.command = None;
        }
    }
    Ok(())
}

fn apply_mcp_server_endpoint(
    config: &mut crate::StudioConfig,
    id: &str,
    endpoint: &str,
) -> pl_protocol::Result<()> {
    let Some(id) = mcp_server_id(id) else {
        return Ok(());
    };
    if crate::config::mcp::is_builtin_mcp_server_id(&id) {
        return Ok(());
    }
    let server = config.mcp.servers.entry(id).or_default();
    let endpoint = endpoint.trim();
    match server.transport {
        pl_tool::mcp::config::McpServerTransport::Stdio => {
            server.command = (!endpoint.is_empty()).then(|| endpoint.to_string());
            server.url = None;
        }
        pl_tool::mcp::config::McpServerTransport::StreamableHttp => {
            server.url = (!endpoint.is_empty()).then(|| endpoint.to_string());
            server.command = None;
        }
    }
    Ok(())
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
