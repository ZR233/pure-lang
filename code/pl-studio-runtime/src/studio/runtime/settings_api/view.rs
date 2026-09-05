//! Settings 只读投影：从 StudioConfig 派生 secret-free 的 snapshot 视图与展示标签。

use anyhow::{Context, Result};
use pl_model::completion::WebSearchMode;
use pl_model::model::ModelInfo;
use pl_model::provider::{ProviderConnectionMode, ProviderWireProtocol};
use pl_protocol::WebSearchContextSize;
use pl_protocol::studio::{
    StudioCustomModelSettings, StudioDeepSeekWebSearchSettings, StudioGeneralSettings,
    StudioInstructionsSettings, StudioMcpServerSettings, StudioModelConnectionSettings,
    StudioProviderSettings, StudioRoleSettings, StudioSettings, StudioSettingsSnapshot,
    StudioSkillsSettings, StudioWebSearchSettings,
};

use crate::{
    ConfigRuntimeSnapshot, ProviderCapabilitySelection, ProviderModelCatalogConfig, StudioRole,
    WebSearchAvailability, WebSearchBackendKind,
};

pub(crate) fn settings_snapshot(state: ConfigRuntimeSnapshot) -> Result<StudioSettingsSnapshot> {
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
            ProviderWireProtocol::Responses => "responses",
            ProviderWireProtocol::ChatCompletions => "chat_completions",
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

pub(super) fn normalized_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(super) fn normalized_string_list(values: Vec<String>) -> Vec<String> {
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
