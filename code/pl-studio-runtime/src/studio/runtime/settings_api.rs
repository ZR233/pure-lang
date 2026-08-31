use anyhow::{Context, Result};
use pl_protocol::studio::{
    ProviderModelConnectionUpdate, ProviderModelUpdate, ProviderSecretUpdate,
    ProviderSettingsUpdate, RoleSettingsUpdate, SetModelRoleRequest, StudioCustomModelSettings,
    StudioDeepSeekWebSearchSettings, StudioError, StudioGeneralSettings,
    StudioInstructionsSettings, StudioMcpServerSettings, StudioModelConnectionSettings,
    StudioProviderSettings, StudioRoleSettings, StudioSettings, StudioSettingsSnapshot,
    StudioSkillsSettings, StudioWebSearchSettings, UpdateDeepSeekWebSearchSettingsRequest,
    UpdateGeneralSettingsRequest, UpdateInstructionsSettingsRequest, UpdateMcpSettingsRequest,
    UpdatePermissionSettingsRequest, UpdateProviderSettingsRequest, UpdateSkillsSettingsRequest,
    UpdateWebSearchSettingsRequest,
};

use crate::{
    ConfigRuntimeSnapshot, ModelInfo, PermissionMode, PromptCacheDialect,
    PromptCacheProviderCapabilities, ProviderCapabilitySelection, ProviderConnectionMode,
    ProviderEdit, ProviderModelCatalogConfig, ProviderModelEdit, ProviderPresetId,
    ProviderServiceCapabilities, ProviderSettingsEdit, ProviderWireProtocol,
    ResponsesHostedToolCapabilities, RoleEdit, StandaloneWebSearchDialect, StudioRole,
    WebSearchAvailability, WebSearchBackendKind, WebSearchContextSize, WebSearchMode,
    WebSearchProviderCapabilities,
};

use super::StudioRuntime;

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
                .map(|provider| provider_edit(provider, &current.config))
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
}

pub(super) fn settings_snapshot(state: ConfigRuntimeSnapshot) -> Result<StudioSettingsSnapshot> {
    let settings = settings_view(&state.config, StudioRole::Executor)?;
    Ok(StudioSettingsSnapshot {
        revision: state.revision,
        updated_at: state.updated_at,
        settings,
    })
}

fn settings_view(
    config: &crate::StudioConfig,
    web_search_role: StudioRole,
) -> Result<StudioSettings> {
    let providers = config
        .models
        .providers
        .iter()
        .map(|(id, provider)| {
            let models = provider.effective_models()?;
            let default_model = config
                .models
                .routes
                .values()
                .find(|route| route.provider == *id)
                .map(|route| route.model.clone())
                .or_else(|| models.first().map(|model| model.slug.clone()))
                .unwrap_or_default();
            let service_capabilities = provider.service_capabilities()?;
            let catalog_id = match &provider.catalog {
                ProviderModelCatalogConfig::Bundled { catalog, .. } => Some(catalog.to_string()),
                ProviderModelCatalogConfig::Explicit { .. } => None,
            };
            Ok(StudioProviderSettings {
                id: id.to_string(),
                template_kind: provider
                    .preset_id()
                    .map(|preset| preset.to_string())
                    .unwrap_or_default(),
                name: provider.name.clone(),
                base_url: provider.base_url.clone(),
                has_bearer_token: provider.resolved_bearer_token().is_some(),
                capability_source: match &provider.capabilities {
                    ProviderCapabilitySelection::PresetDefaults => "preset_defaults",
                    ProviderCapabilitySelection::Explicit(_) => "explicit",
                }
                .to_string(),
                hosted_web_search: service_capabilities.web_search.hosted_responses,
                hosted_web_search_dialect: service_capabilities
                    .web_search
                    .hosted_dialect
                    .as_str()
                    .to_string(),
                standalone_web_search: service_capabilities
                    .web_search
                    .standalone
                    .map(|dialect| dialect.as_str().to_string()),
                prompt_cache_dialect: service_capabilities
                    .prompt_cache
                    .dialect
                    .as_str()
                    .to_string(),
                responses_programmatic_tool_calling: service_capabilities
                    .responses_tools
                    .programmatic_tool_calling,
                default_model,
                custom_models: provider
                    .editable_models()
                    .iter()
                    .map(custom_model_settings)
                    .collect(),
                model_connection_modes: provider
                    .connection_overrides()
                    .iter()
                    .map(|(slug, mode)| StudioModelConnectionSettings {
                        slug: slug.clone(),
                        connection_mode: connection_mode_label(*mode).to_string(),
                    })
                    .collect(),
                catalog_id,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let roles = StudioRole::all()
        .into_iter()
        .map(|role| {
            let route = config
                .models
                .routes
                .get(&role.id())
                .with_context(|| format!("missing Studio role route: {}", role.key()))?;
            Ok(StudioRoleSettings {
                key: role.key().to_string(),
                provider_id: route.provider.to_string(),
                model: route.model.clone(),
                effort: route
                    .effort
                    .as_ref()
                    .map(|effort| effort.as_str().to_string())
                    .unwrap_or_default(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let mcp_servers = crate::config::effective_mcp_servers(config)
        .into_values()
        .map(|server| StudioMcpServerSettings {
            id: server.id,
            transport: server.config.transport.as_str().to_string(),
            endpoint: server.config.endpoint_summary(),
            configuration: match server.status_kind {
                pl_core::McpServerStatusKind::Enabled => {
                    pl_protocol::studio::StudioMcpServerConfiguration::Enabled
                }
                pl_core::McpServerStatusKind::Disabled => {
                    pl_protocol::studio::StudioMcpServerConfiguration::Disabled
                }
                pl_core::McpServerStatusKind::MissingCredential => {
                    pl_protocol::studio::StudioMcpServerConfiguration::MissingCredential
                }
            },
            source_kind: server.source_kind.as_str().to_string(),
            mutation_policy: server.mutation_policy.as_str().to_string(),
        })
        .collect();
    let (web_search, deepseek_web_search) = search_settings(config, web_search_role)?;
    Ok(StudioSettings {
        default_provider_id: config
            .models
            .routes
            .get(&StudioRole::Planner.id())
            .map(|route| route.provider.to_string()),
        providers,
        roles,
        permission_mode: config.runtime.permission_mode.label().to_string(),
        instructions: StudioInstructionsSettings {
            base_override: config.instructions.base_override.clone(),
            developer: config.instructions.developer.clone(),
            user: config.instructions.user.clone(),
            project_doc_max_bytes: config.instructions.project_doc_max_bytes as u64,
            project_doc_fallback_filenames: config
                .instructions
                .project_doc_fallback_filenames
                .clone(),
        },
        skills: StudioSkillsSettings {
            enabled: config.skills.enabled,
            auto_learn: config.skills.auto_learn,
            system_enabled: config.skills.system.enabled,
            project_dir: config.skills.project_dir.clone(),
            user_dir: config.skills.user_dir.clone(),
            external_dirs: config.skills.external_dirs.clone(),
            disabled: config.skills.disabled.clone(),
            auto_learn_min_tool_calls: config.skills.auto_learn_min_tool_calls,
        },
        mcp_servers,
        general: StudioGeneralSettings {
            follow_system_theme: config.ui.follow_system_theme,
            follow_active_turn: config.ui.follow_active_turn,
            compact_timeline: config.ui.compact_timeline,
        },
        web_search,
        deepseek_web_search,
    })
}

fn custom_model_settings(model: &ModelInfo) -> StudioCustomModelSettings {
    let reasoning_efforts = model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "effort")
        .map(|parameter| parameter.candidates.as_slice())
        .unwrap_or_default();
    StudioCustomModelSettings {
        slug: model.slug.clone(),
        display_name: model.display_name.clone(),
        reasoning_efforts: reasoning_efforts.to_vec(),
        base_instructions: model.base_instructions.clone(),
        wire_protocol: match model.transport.protocol {
            crate::ProviderWireProtocol::Responses => "responses",
            crate::ProviderWireProtocol::ChatCompletions => "chat_completions",
        }
        .to_string(),
        supported_connection_modes: model
            .transport
            .supported_connection_modes
            .iter()
            .copied()
            .map(connection_mode_label)
            .map(str::to_string)
            .collect(),
        default_connection_mode: connection_mode_label(model.transport.default_connection_mode)
            .to_string(),
    }
}

fn search_settings(
    config: &crate::StudioConfig,
    role: StudioRole,
) -> Result<(StudioWebSearchSettings, StudioDeepSeekWebSearchSettings)> {
    let route = config.resolve_role(role)?;
    let plans = crate::plan_web_searches(
        &config.models,
        &route,
        &config.web_search,
        config.deepseek_web_search.enabled,
    )?;
    let openai = plans.openai.resolution;
    let deepseek = plans.deepseek.resolution;
    let openai_selected = plans.selected == Some(WebSearchBackendKind::OpenAi);
    let deepseek_selected = plans.selected == Some(WebSearchBackendKind::DeepSeek);
    let location = config.web_search.location.as_ref();
    Ok((
        StudioWebSearchSettings {
            configured_mode: web_search_mode_label(openai.configured_mode).to_string(),
            effective_mode: web_search_mode_label(if openai_selected {
                openai.effective_mode
            } else {
                WebSearchMode::Disabled
            })
            .to_string(),
            availability: web_search_availability_label(openai.availability).to_string(),
            selected: openai_selected,
            context_size: config
                .web_search
                .context_size
                .map(|size| match size {
                    WebSearchContextSize::Low => "low",
                    WebSearchContextSize::Medium => "medium",
                    WebSearchContextSize::High => "high",
                })
                .map(str::to_string),
            allowed_domains: config.web_search.allowed_domains.clone(),
            country: location.and_then(|location| location.country.clone()),
            region: location.and_then(|location| location.region.clone()),
            city: location.and_then(|location| location.city.clone()),
            timezone: location.and_then(|location| location.timezone.clone()),
            provider_id: openai.provider_id.map(|provider| provider.to_string()),
            model: openai.model,
        },
        StudioDeepSeekWebSearchSettings {
            configured_enabled: config.deepseek_web_search.enabled,
            effective_enabled: deepseek_selected,
            availability: web_search_availability_label(deepseek.availability).to_string(),
            selected: deepseek_selected,
            provider_id: deepseek.provider_id.map(|provider| provider.to_string()),
            model: deepseek.model,
        },
    ))
}

fn web_search_availability_label(availability: WebSearchAvailability) -> &'static str {
    match availability {
        WebSearchAvailability::Available => "available",
        WebSearchAvailability::Disabled => "disabled",
        WebSearchAvailability::MissingCredential => "missingCredential",
        WebSearchAvailability::ProviderUnsupported => "providerUnsupported",
        WebSearchAvailability::ModelUnsupported => "modelUnsupported",
    }
}

fn web_search_config(
    request: UpdateWebSearchSettingsRequest,
) -> Result<(u64, pl_model::WebSearchConfig)> {
    let mode = match request.mode.trim() {
        "disabled" => WebSearchMode::Disabled,
        "cached" => WebSearchMode::Cached,
        "indexed" => WebSearchMode::Indexed,
        "live" => WebSearchMode::Live,
        _ => return Err(invalid_settings_argument("Unsupported web search mode")),
    };
    let context_size = match request.context_size.as_deref().map(str::trim) {
        None | Some("") => None,
        Some("low") => Some(WebSearchContextSize::Low),
        Some("medium") => Some(WebSearchContextSize::Medium),
        Some("high") => Some(WebSearchContextSize::High),
        Some(_) => {
            return Err(invalid_settings_argument(
                "Unsupported web search context size",
            ));
        }
    };
    let location = crate::WebSearchLocation {
        country: normalized_optional(request.country),
        region: normalized_optional(request.region),
        city: normalized_optional(request.city),
        timezone: normalized_optional(request.timezone),
    };
    Ok((
        request.expected_revision,
        pl_model::WebSearchConfig {
            mode,
            context_size,
            allowed_domains: normalized_string_list(request.allowed_domains),
            location: (!location.is_empty()).then_some(location),
        },
    ))
}

fn normalized_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn normalized_string_list(values: Vec<String>) -> Vec<String> {
    let mut values = values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values
}

fn web_search_mode_label(mode: WebSearchMode) -> &'static str {
    match mode {
        WebSearchMode::Disabled => "disabled",
        WebSearchMode::Cached => "cached",
        WebSearchMode::Indexed => "indexed",
        WebSearchMode::Live => "live",
    }
}

fn connection_mode_label(mode: ProviderConnectionMode) -> &'static str {
    match mode {
        ProviderConnectionMode::WebSocket => "web_socket",
        ProviderConnectionMode::Http => "http",
    }
}

fn provider_edit(
    input: ProviderSettingsUpdate,
    current: &crate::StudioConfig,
) -> Result<ProviderEdit> {
    let preset = (!input.template_kind.trim().is_empty())
        .then(|| ProviderPresetId::new(input.template_kind.trim()))
        .transpose()
        .map_err(|_| invalid_settings_argument("Invalid provider preset id"))?;
    let current_id = input.original_id.as_deref().unwrap_or(&input.id);
    let current_token = current
        .models
        .providers
        .get(&crate::ProviderId::new(current_id)?)
        .and_then(|provider| provider.bearer_token.clone());
    let bearer_token = match input.secret {
        ProviderSecretUpdate::Preserve => current_token,
        ProviderSecretUpdate::Replace { value } => {
            let value = value.trim();
            if value.is_empty() {
                return Err(invalid_settings_argument(
                    "Replacement provider credential cannot be empty",
                ));
            }
            Some(value.to_string())
        }
        ProviderSecretUpdate::Clear => None,
    };
    let capabilities =
        match input.capability_source.as_str() {
            "preset_defaults" if preset.is_some() => ProviderCapabilitySelection::PresetDefaults,
            "preset_defaults" => {
                return Err(invalid_settings_argument(
                    "Custom providers must use explicit service capabilities",
                ));
            }
            "explicit" => ProviderCapabilitySelection::Explicit(ProviderServiceCapabilities {
                web_search: WebSearchProviderCapabilities {
                    hosted_responses: input.hosted_web_search,
                    hosted_dialect: input.hosted_web_search_dialect.trim().parse().map_err(
                        |_| invalid_settings_argument("Unsupported hosted web search dialect"),
                    )?,
                    standalone: input
                        .standalone_web_search
                        .as_deref()
                        .filter(|value| !value.trim().is_empty())
                        .map(str::parse::<StandaloneWebSearchDialect>)
                        .transpose()
                        .map_err(|_| {
                            invalid_settings_argument("Unsupported standalone web search dialect")
                        })?,
                },
                prompt_cache: PromptCacheProviderCapabilities {
                    dialect: input
                        .prompt_cache_dialect
                        .trim()
                        .parse::<PromptCacheDialect>()
                        .map_err(|_| {
                            invalid_settings_argument("Unsupported prompt cache dialect")
                        })?,
                },
                responses_tools: ResponsesHostedToolCapabilities {
                    programmatic_tool_calling: input.responses_programmatic_tool_calling,
                },
            }),
            _ => {
                return Err(invalid_settings_argument(
                    "Unsupported provider capability source",
                ));
            }
        };
    Ok(ProviderEdit {
        key: input.id,
        original_key: input.original_id,
        preset,
        name: input.name,
        base_url: Some(input.base_url),
        bearer_token,
        capabilities,
        default_model: input.default_model,
        custom_models: input
            .custom_models
            .into_iter()
            .map(provider_model_edit)
            .collect::<Result<Vec<_>>>()?,
        model_connection_modes: model_connection_modes(input.model_connection_modes)?,
    })
}

fn provider_model_edit(input: ProviderModelUpdate) -> Result<ProviderModelEdit> {
    Ok(ProviderModelEdit {
        slug: input.slug,
        display_name: input.display_name,
        efforts: input.reasoning_efforts,
        base_instructions: input.base_instructions.unwrap_or_default(),
        protocol: parse_provider_protocol(&input.wire_protocol)?,
        supported_connection_modes: input
            .supported_connection_modes
            .iter()
            .map(|mode| parse_provider_connection_mode(mode))
            .collect::<Result<Vec<_>>>()?,
        default_connection_mode: parse_provider_connection_mode(&input.default_connection_mode)?,
    })
}

fn model_connection_modes(
    inputs: Vec<ProviderModelConnectionUpdate>,
) -> Result<std::collections::BTreeMap<String, ProviderConnectionMode>> {
    let mut modes = std::collections::BTreeMap::new();
    for input in inputs {
        let slug = input.slug.trim();
        if slug.is_empty() {
            return Err(invalid_settings_argument(
                "Model connection slug must not be empty",
            ));
        }
        if modes
            .insert(
                slug.to_string(),
                parse_provider_connection_mode(&input.connection_mode)?,
            )
            .is_some()
        {
            return Err(invalid_settings_argument("Duplicate model connection mode"));
        }
    }
    Ok(modes)
}

fn parse_provider_protocol(value: &str) -> Result<ProviderWireProtocol> {
    match value.trim() {
        "responses" => Ok(ProviderWireProtocol::Responses),
        "chat_completions" => Ok(ProviderWireProtocol::ChatCompletions),
        _ => Err(invalid_settings_argument("Unsupported model wire protocol")),
    }
}

fn parse_provider_connection_mode(value: &str) -> Result<ProviderConnectionMode> {
    match value.trim() {
        "web_socket" => Ok(ProviderConnectionMode::WebSocket),
        "http" => Ok(ProviderConnectionMode::Http),
        _ => Err(invalid_settings_argument(
            "Unsupported model connection mode",
        )),
    }
}

fn invalid_settings_argument(message: &'static str) -> anyhow::Error {
    anyhow::Error::new(StudioError::invalid_argument(message))
}

impl From<RoleSettingsUpdate> for RoleEdit {
    fn from(input: RoleSettingsUpdate) -> Self {
        Self {
            key: input.key,
            provider: input.provider,
            model: input.model,
            effort: input.effort,
        }
    }
}
