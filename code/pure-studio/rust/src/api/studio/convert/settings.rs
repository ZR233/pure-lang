use crate::api::studio::types::{
    BridgeGeneralSettingsDto, BridgeInstructionsSettingsDto, BridgeMcpServerSettingsDto,
    BridgeProviderModelSettingsDto, BridgeProviderSettingsDto, BridgeRoleSettingsDto,
    BridgeSkillsSettingsDto, BridgeStudioSettingsDto, DeepSeekBalanceDto, DeepSeekBalanceInfoDto,
    ProviderInput, ProviderModelConnectionInput, ProviderModelInput, ProviderSecretInput,
    ProviderSettingsInput, ProviderUsageDto, RoleInput, ZhipuCodingPlanUsageDto,
    ZhipuQuotaLimitDto, ZhipuToolUsageDetailDto,
};
use anyhow::{Context, Result};
use pl_studio_runtime::{
    McpServerTransport, ModelInfo, PromptCacheDialect, PromptCacheProviderCapabilities,
    ProviderCapabilitySelection, ProviderEdit, ProviderModelCatalogConfig, ProviderModelEdit,
    ProviderPresetId, ProviderServiceCapabilities, ProviderSettingsEdit, ProviderUsageData,
    ProviderUsageState, ProviderWireProtocol, ResponsesHostedToolCapabilities, RoleEdit,
    StandaloneWebSearchDialect, StudioRole, WebSearchProviderCapabilities, ZhipuQuotaWindow,
};
// ── Utility functions ──

pub(crate) fn normalized_string_list(values: Vec<String>) -> Vec<String> {
    let mut values = values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values
}

pub(crate) fn mcp_transport_from_label(label: &str) -> Result<McpServerTransport> {
    match label.trim() {
        "stdio" => Ok(McpServerTransport::Stdio),
        "streamableHttp" => Ok(McpServerTransport::StreamableHttp),
        label => anyhow::bail!("unsupported MCP transport: {label}"),
    }
}

/// 构造 Flutter 使用的无 secret canonical typed 设置快照。
pub(crate) fn studio_settings_dto(
    config: &pl_studio_runtime::StudioConfig,
    general: BridgeGeneralSettingsDto,
    web_search_role: StudioRole,
) -> Result<BridgeStudioSettingsDto> {
    let providers = config
        .models
        .providers
        .iter()
        .map(|(id, provider)| {
            let declared_models = provider.declared_models()?;
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
            Ok(BridgeProviderSettingsDto {
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
                standalone_web_search: service_capabilities
                    .web_search
                    .standalone
                    .map(|dialect| dialect.as_str().to_string()),
                prompt_cache_dialect: service_capabilities
                    .prompt_cache
                    .dialect
                    .as_str()
                    .to_string(),
                responses_tool_search: service_capabilities.responses_tools.tool_search,
                responses_programmatic_tool_calling: service_capabilities
                    .responses_tools
                    .programmatic_tool_calling,
                default_model,
                models: declared_models
                    .iter()
                    .map(|model| {
                        let current = models
                            .iter()
                            .find(|candidate| candidate.slug == model.slug)
                            .unwrap_or(model);
                        model_settings_dto(model, current)
                    })
                    .collect(),
                custom_models: provider
                    .editable_models()
                    .iter()
                    .map(|model| {
                        let current = models
                            .iter()
                            .find(|candidate| candidate.slug == model.slug)
                            .unwrap_or(model);
                        model_settings_dto(model, current)
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
            Ok(BridgeRoleSettingsDto {
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
    let mcp_servers = pl_studio_runtime::config::effective_mcp_servers(config)
        .into_values()
        .map(|server| BridgeMcpServerSettingsDto {
            id: server.id,
            transport: server.config.transport.as_str().to_string(),
            endpoint: server.config.endpoint_summary(),
            enabled: server.config.enabled,
            status: server.status_kind.as_str().to_string(),
            source_kind: server.source_kind.as_str().to_string(),
            mutation_policy: server.mutation_policy.as_str().to_string(),
        })
        .collect();

    Ok(BridgeStudioSettingsDto {
        default_provider_id: config
            .models
            .routes
            .get(&StudioRole::Planner.id())
            .map(|route| route.provider.to_string()),
        providers,
        roles,
        permission_mode: config.runtime.permission_mode.label().to_string(),
        instructions: BridgeInstructionsSettingsDto {
            base_override: config.instructions.base_override.clone(),
            developer: config.instructions.developer.clone(),
            user: config.instructions.user.clone(),
            project_doc_max_bytes: config.instructions.project_doc_max_bytes as u64,
            project_doc_fallback_filenames: config
                .instructions
                .project_doc_fallback_filenames
                .clone(),
        },
        skills: BridgeSkillsSettingsDto {
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
        general,
        web_search: web_search_settings_dto(config, web_search_role)?,
    })
}

fn model_settings_dto(model: &ModelInfo, current: &ModelInfo) -> BridgeProviderModelSettingsDto {
    let reasoning_efforts = model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "effort")
        .map(|parameter| parameter.candidates.as_slice())
        .unwrap_or_default();
    BridgeProviderModelSettingsDto {
        slug: model.slug.clone(),
        display_name: model.display_name.clone(),
        description: model.description.clone().unwrap_or_default(),
        context_window: model.context_window,
        max_output_tokens: model.max_output_tokens,
        currency: model.currency.clone().unwrap_or_default(),
        input_price_per_m_tok: model.input_price_per_mtok,
        output_price_per_m_tok: model.output_price_per_mtok,
        cache_read_price_per_m_tok: model.cache_read_price_per_mtok,
        cache_write_price_per_m_tok: model.cache_write_price_per_mtok,
        reasoning_efforts: reasoning_efforts.to_vec(),
        base_instructions: model.base_instructions.clone(),
        wire_protocol: protocol_label(model.transport.protocol).to_string(),
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
        connection_mode: connection_mode_label(current.transport.default_connection_mode)
            .to_string(),
    }
}

pub(crate) fn web_search_settings_dto(
    config: &pl_studio_runtime::StudioConfig,
    role: pl_studio_runtime::StudioRole,
) -> Result<crate::api::studio::types::BridgeWebSearchSettingsDto> {
    let route = config.resolve_role(role)?;
    let resolution =
        pl_studio_runtime::plan_web_search(&config.models, &route, &config.web_search)?.resolution;
    let location = config.web_search.location.as_ref();
    Ok(crate::api::studio::types::BridgeWebSearchSettingsDto {
        configured_mode: web_search_mode_label(resolution.configured_mode).to_string(),
        effective_mode: web_search_mode_label(resolution.effective_mode).to_string(),
        availability: web_search_availability_label(resolution.availability).to_string(),
        context_size: config
            .web_search
            .context_size
            .map(web_search_context_size_label)
            .map(str::to_string),
        allowed_domains: config.web_search.allowed_domains.clone(),
        country: location.and_then(|location| location.country.clone()),
        region: location.and_then(|location| location.region.clone()),
        city: location.and_then(|location| location.city.clone()),
        timezone: location.and_then(|location| location.timezone.clone()),
        provider_id: resolution
            .provider_id
            .map(|provider_id| provider_id.to_string()),
        model: resolution.model,
    })
}

pub(crate) fn web_search_config_from_input(
    input: crate::api::studio::types::WebSearchSettingsInput,
) -> Result<pl_studio_runtime::StudioWebSearchConfig> {
    let mode = match input.mode.trim() {
        "disabled" => pl_studio_runtime::WebSearchMode::Disabled,
        "cached" => pl_studio_runtime::WebSearchMode::Cached,
        "indexed" => pl_studio_runtime::WebSearchMode::Indexed,
        "live" => pl_studio_runtime::WebSearchMode::Live,
        mode => anyhow::bail!("unsupported web search mode: {mode}"),
    };
    let context_size = match input.context_size.as_deref().map(str::trim) {
        None | Some("") => None,
        Some("low") => Some(pl_studio_runtime::WebSearchContextSize::Low),
        Some("medium") => Some(pl_studio_runtime::WebSearchContextSize::Medium),
        Some("high") => Some(pl_studio_runtime::WebSearchContextSize::High),
        Some(size) => anyhow::bail!("unsupported web search context size: {size}"),
    };
    let location = pl_studio_runtime::WebSearchLocation {
        country: normalized_optional(input.country),
        region: normalized_optional(input.region),
        city: normalized_optional(input.city),
        timezone: normalized_optional(input.timezone),
    };
    Ok(pl_studio_runtime::StudioWebSearchConfig {
        mode,
        context_size,
        allowed_domains: normalized_string_list(input.allowed_domains),
        location: (!location.is_empty()).then_some(location),
    })
}

fn normalized_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn web_search_mode_label(mode: pl_studio_runtime::WebSearchMode) -> &'static str {
    match mode {
        pl_studio_runtime::WebSearchMode::Disabled => "disabled",
        pl_studio_runtime::WebSearchMode::Cached => "cached",
        pl_studio_runtime::WebSearchMode::Indexed => "indexed",
        pl_studio_runtime::WebSearchMode::Live => "live",
    }
}

fn web_search_context_size_label(size: pl_studio_runtime::WebSearchContextSize) -> &'static str {
    match size {
        pl_studio_runtime::WebSearchContextSize::Low => "low",
        pl_studio_runtime::WebSearchContextSize::Medium => "medium",
        pl_studio_runtime::WebSearchContextSize::High => "high",
    }
}

fn web_search_availability_label(
    availability: pl_studio_runtime::WebSearchAvailability,
) -> &'static str {
    match availability {
        pl_studio_runtime::WebSearchAvailability::Available => "available",
        pl_studio_runtime::WebSearchAvailability::Disabled => "disabled",
        pl_studio_runtime::WebSearchAvailability::MissingCredential => "missingCredential",
        pl_studio_runtime::WebSearchAvailability::ProviderUnsupported => "providerUnsupported",
        pl_studio_runtime::WebSearchAvailability::ModelUnsupported => "modelUnsupported",
    }
}

fn protocol_label(protocol: pl_studio_runtime::ProviderWireProtocol) -> &'static str {
    match protocol {
        pl_studio_runtime::ProviderWireProtocol::Responses => "responses",
        pl_studio_runtime::ProviderWireProtocol::ChatCompletions => "chat_completions",
    }
}

fn connection_mode_label(mode: pl_studio_runtime::ProviderConnectionMode) -> &'static str {
    match mode {
        pl_studio_runtime::ProviderConnectionMode::WebSocket => "web_socket",
        pl_studio_runtime::ProviderConnectionMode::Http => "http",
    }
}

// ── Provider settings converters ──

pub(crate) fn provider_settings_edit(
    input: ProviderSettingsInput,
    current: &pl_studio_runtime::StudioConfig,
) -> Result<ProviderSettingsEdit> {
    Ok(ProviderSettingsEdit {
        default_provider: Some(input.default_provider_id),
        providers: input
            .providers
            .into_iter()
            .map(|provider| provider_edit(provider, current))
            .collect::<Result<Vec<_>>>()?,
        roles: input.roles.into_iter().map(RoleEdit::from).collect(),
    })
}

fn provider_edit(
    input: ProviderInput,
    current: &pl_studio_runtime::StudioConfig,
) -> Result<ProviderEdit> {
    let preset = (!input.template_kind.trim().is_empty())
        .then(|| ProviderPresetId::new(input.template_kind.trim()))
        .transpose()
        .context("invalid provider preset id")?;
    let current_id = input.original_id.as_deref().unwrap_or(&input.id);
    let current_token = current
        .models
        .providers
        .get(&pl_studio_runtime::ProviderId::new(current_id)?)
        .and_then(|provider| provider.bearer_token.clone());
    let bearer_token = match input.secret {
        ProviderSecretInput::Preserve => current_token,
        ProviderSecretInput::Replace { value } => {
            let value = value.trim();
            if value.is_empty() {
                anyhow::bail!("replacement provider credential cannot be empty");
            }
            Some(value.to_string())
        }
        ProviderSecretInput::Clear => None,
    };
    let capabilities = match input.capability_source.as_str() {
        "preset_defaults" if preset.is_some() => ProviderCapabilitySelection::PresetDefaults,
        "preset_defaults" => {
            anyhow::bail!("custom provider must use explicit service capabilities")
        }
        "explicit" => ProviderCapabilitySelection::Explicit(ProviderServiceCapabilities {
            web_search: WebSearchProviderCapabilities {
                hosted_responses: input.hosted_web_search,
                standalone: input
                    .standalone_web_search
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                    .map(str::parse::<StandaloneWebSearchDialect>)
                    .transpose()
                    .map_err(anyhow::Error::msg)?,
            },
            prompt_cache: PromptCacheProviderCapabilities {
                dialect: input
                    .prompt_cache_dialect
                    .trim()
                    .parse::<PromptCacheDialect>()
                    .map_err(anyhow::Error::msg)?,
            },
            responses_tools: ResponsesHostedToolCapabilities {
                tool_search: input.responses_tool_search,
                programmatic_tool_calling: input.responses_programmatic_tool_calling,
            },
        }),
        source => anyhow::bail!("unsupported provider capability source: {source}"),
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

fn provider_model_edit(input: ProviderModelInput) -> Result<ProviderModelEdit> {
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
    inputs: Vec<ProviderModelConnectionInput>,
) -> Result<std::collections::BTreeMap<String, pl_studio_runtime::ProviderConnectionMode>> {
    let mut modes = std::collections::BTreeMap::new();
    for input in inputs {
        let slug = input.slug.trim();
        if slug.is_empty() {
            anyhow::bail!("model connection slug must not be empty");
        }
        if modes
            .insert(
                slug.to_string(),
                parse_provider_connection_mode(&input.connection_mode)?,
            )
            .is_some()
        {
            anyhow::bail!("duplicate model connection mode: {slug}");
        }
    }
    Ok(modes)
}

fn parse_provider_protocol(value: &str) -> Result<ProviderWireProtocol> {
    match value.trim() {
        "responses" => Ok(ProviderWireProtocol::Responses),
        "chat_completions" => Ok(ProviderWireProtocol::ChatCompletions),
        protocol => anyhow::bail!("unsupported model wire protocol: {protocol}"),
    }
}

fn parse_provider_connection_mode(
    value: &str,
) -> Result<pl_studio_runtime::ProviderConnectionMode> {
    match value.trim() {
        "web_socket" => Ok(pl_studio_runtime::ProviderConnectionMode::WebSocket),
        "http" => Ok(pl_studio_runtime::ProviderConnectionMode::Http),
        mode => anyhow::bail!("unsupported model connection mode: {mode}"),
    }
}

pub(crate) fn provider_usage_dto(
    record: pl_studio_runtime::ProviderUsageRecord,
) -> ProviderUsageDto {
    match record.state {
        ProviderUsageState::Unsupported => ProviderUsageDto {
            provider_id: record.provider_id,
            updated_at: record.updated_at,
            status: "unsupported".to_string(),
            usage_kind: "unsupported".to_string(),
            message: None,
            balance: None,
            coding_plan: None,
        },
        ProviderUsageState::MissingCredential => ProviderUsageDto {
            provider_id: record.provider_id,
            updated_at: record.updated_at,
            status: "missingCredential".to_string(),
            usage_kind: "unknown".to_string(),
            message: Some("provider API key is not configured".to_string()),
            balance: None,
            coding_plan: None,
        },
        ProviderUsageState::Failed(message) => ProviderUsageDto {
            provider_id: record.provider_id,
            updated_at: record.updated_at,
            status: "error".to_string(),
            usage_kind: String::new(),
            message: Some(message),
            balance: None,
            coding_plan: None,
        },
        ProviderUsageState::Ready(ProviderUsageData::DeepSeekBalance(balance)) => {
            ProviderUsageDto {
                provider_id: record.provider_id,
                updated_at: record.updated_at,
                status: "ready".to_string(),
                usage_kind: "deepseekBalance".to_string(),
                message: None,
                balance: Some(DeepSeekBalanceDto {
                    is_available: balance.is_available,
                    balances: balance
                        .balances
                        .into_iter()
                        .map(|item| DeepSeekBalanceInfoDto {
                            currency: item.currency,
                            total_balance: item.total_balance,
                            granted_balance: item.granted_balance,
                            topped_up_balance: item.topped_up_balance,
                        })
                        .collect(),
                }),
                coding_plan: None,
            }
        }
        ProviderUsageState::Ready(ProviderUsageData::ZhipuCodingPlan(usage)) => ProviderUsageDto {
            provider_id: record.provider_id,
            updated_at: record.updated_at,
            status: "ready".to_string(),
            usage_kind: "zhipuCodingPlan".to_string(),
            message: None,
            balance: None,
            coding_plan: Some(ZhipuCodingPlanUsageDto {
                level: usage.level,
                limits: usage
                    .limits
                    .into_iter()
                    .map(|limit| {
                        let (window, label) = zhipu_window_labels(&limit.window);
                        ZhipuQuotaLimitDto {
                            window: window.to_string(),
                            label: label.to_string(),
                            percentage: limit.percentage,
                            current_value: limit.current_value,
                            total: limit.total,
                            remaining: limit.remaining,
                            next_reset_at: limit.next_reset_at,
                            usage_details: limit
                                .usage_details
                                .into_iter()
                                .map(|detail| ZhipuToolUsageDetailDto {
                                    name: detail.name,
                                    current_value: detail.current_value,
                                    total: detail.total,
                                    percentage: detail.percentage,
                                })
                                .collect(),
                        }
                    })
                    .collect(),
            }),
        },
    }
}

fn zhipu_window_labels(window: &ZhipuQuotaWindow) -> (&'static str, &str) {
    match window {
        ZhipuQuotaWindow::FiveHour => ("fiveHour", "5h"),
        ZhipuQuotaWindow::Weekly => ("weekly", "7d"),
        ZhipuQuotaWindow::McpMonthly => ("mcpMonthly", "MCP"),
        ZhipuQuotaWindow::Other(label) => ("other", label.as_str()),
    }
}

impl From<RoleInput> for RoleEdit {
    fn from(input: RoleInput) -> Self {
        Self {
            key: input.key,
            provider: input.provider,
            model: input.model,
            effort: input.effort,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_studio_runtime::{ProviderId, StudioConfig, builtin_provider_catalog};
    use pretty_assertions::assert_eq;

    #[test]
    fn renamed_provider_uses_original_id_to_preserve_secret() {
        let mut current = StudioConfig::default_config();
        let openai = builtin_provider_catalog()
            .presets
            .into_iter()
            .find(|preset| preset.id.as_str() == "openai")
            .unwrap()
            .provider;
        current
            .models
            .providers
            .insert(ProviderId::new("openai").unwrap(), openai);
        current
            .models
            .providers
            .get_mut(&ProviderId::new("openai").unwrap())
            .unwrap()
            .bearer_token = Some("existing-secret".to_string());

        let edit = provider_edit(
            ProviderInput {
                id: "openai-team".to_string(),
                original_id: Some("openai".to_string()),
                template_kind: "openai".to_string(),
                name: "OpenAI Team".to_string(),
                base_url: "https://api.openai.com/v1".to_string(),
                secret: ProviderSecretInput::Preserve,
                capability_source: "preset_defaults".to_string(),
                hosted_web_search: true,
                standalone_web_search: Some("open_ai_search_api".to_string()),
                prompt_cache_dialect: "open_ai_prompt_cache_key".to_string(),
                responses_tool_search: true,
                responses_programmatic_tool_calling: true,
                default_model: "gpt-5.6-sol".to_string(),
                custom_models: Vec::new(),
                model_connection_modes: Vec::new(),
            },
            &current,
        )
        .unwrap();

        assert_eq!(edit.original_key.as_deref(), Some("openai"));
        assert_eq!(edit.bearer_token.as_deref(), Some("existing-secret"));

        let next = ProviderSettingsEdit {
            default_provider: Some("openai-team".to_string()),
            providers: vec![edit],
            roles: Vec::new(),
        }
        .to_config(&current)
        .unwrap();
        assert_eq!(
            next.models
                .providers
                .get(&ProviderId::new("openai-team").unwrap())
                .unwrap()
                .bearer_token
                .as_deref(),
            Some("existing-secret")
        );
    }
}
