use crate::api::studio::types::{
    DeepSeekBalanceDto, DeepSeekBalanceInfoDto, ProviderInput, ProviderModelInput,
    ProviderSecretInput, ProviderSettingsInput, ProviderUsageDto, RoleInput,
    ZhipuCodingPlanUsageDto, ZhipuQuotaLimitDto, ZhipuToolUsageDetailDto,
};
use anyhow::{Context, Result};
use pl_studio_runtime::{
    McpServerTransport, ModelInfo, ProviderCapabilitySelection, ProviderEdit,
    ProviderModelCatalogConfig, ProviderModelEdit, ProviderPresetId, ProviderServiceCapabilities,
    ProviderSettingsEdit, ProviderUsageData, ProviderUsageState, ProviderWireProtocol, RoleEdit,
    StandaloneWebSearchDialect, WebSearchProviderCapabilities, ZhipuQuotaWindow,
};
use serde_json::{Map, Value, json};
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

/// 构造 Flutter 使用的无 secret 配置 projection，并注入服务端解析后的模型目录。
pub(crate) fn studio_config_projection(config: &pl_studio_runtime::StudioConfig) -> Result<Value> {
    let mut providers = Map::new();
    for (id, provider) in &config.models.providers {
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
        providers.insert(
            id.to_string(),
            json!({
                "presetId": provider.preset_id().map(ToString::to_string),
                "wireProtocol": protocol_label(provider.protocol()?),
                "connectionMode": connection_mode_label(provider.connection_mode()),
                "name": provider.name,
                "baseUrl": provider.base_url,
                "hasBearerToken": provider.resolved_bearer_token().is_some(),
                "capabilitySource": match &provider.capabilities {
                    ProviderCapabilitySelection::PresetDefaults => "preset_defaults",
                    ProviderCapabilitySelection::Explicit(_) => "explicit",
                },
                "serviceCapabilities": {
                    "webSearch": {
                        "hostedResponses": service_capabilities.web_search.hosted_responses,
                        "standalone": service_capabilities
                            .web_search
                            .standalone
                            .map(|dialect| dialect.as_str()),
                    },
                },
                "defaultModel": default_model,
                "models": models.iter().map(model_projection).collect::<Vec<_>>(),
                "customModels": provider
                    .editable_models()
                    .iter()
                    .map(model_projection)
                    .collect::<Vec<_>>(),
                "catalogId": catalog_id,
            }),
        );
    }
    let roles = config
        .models
        .routes
        .iter()
        .map(|(role, route)| {
            (
                role.to_string(),
                json!({
                    "provider": route.provider,
                    "model": route.model,
                    "effort": route.effort.as_ref().map(|effort| effort.as_str()),
                }),
            )
        })
        .collect::<Map<_, _>>();
    let default_provider_id = config
        .models
        .routes
        .get(&pl_studio_runtime::StudioRole::Planner.id())
        .map(|route| route.provider.to_string());
    let mcp_servers = config
        .mcp
        .servers
        .iter()
        .map(|(id, server)| {
            (
                id.clone(),
                json!({
                    "enabled": server.enabled,
                    "transport": mcp_transport_label(server.transport),
                    "command": server.command,
                    "url": server.url,
                }),
            )
        })
        .collect::<Map<_, _>>();
    let builtin_mcp_servers = config
        .mcp
        .builtin_servers
        .iter()
        .map(|(id, server)| {
            (
                id.clone(),
                json!({
                    "enabled": server.enabled,
                }),
            )
        })
        .collect::<Map<_, _>>();
    Ok(json!({
        "schemaVersion": config.schema_version,
        "defaultProviderId": default_provider_id,
        "providers": providers,
        "roles": roles,
        "runtime": {
            "permissionMode": config.runtime.permission_mode.label(),
            "activeSkills": config.runtime.active_skills,
            "activeMcpServers": config.runtime.active_mcp_servers,
            "openAiCompactionMode": openai_compaction_mode_label(
                config.runtime.openai_compaction_mode,
            ),
        },
        "instructions": {
            "baseOverride": config.instructions.base_override,
            "developer": config.instructions.developer,
            "user": config.instructions.user,
            "projectDocMaxBytes": config.instructions.project_doc_max_bytes,
            "projectDocFallbackFilenames": config.instructions.project_doc_fallback_filenames,
        },
        "skills": {
            "enabled": config.skills.enabled,
            "autoLearn": config.skills.auto_learn,
            "system": {
                "enabled": config.skills.system.enabled,
            },
            "projectDir": config.skills.project_dir,
            "userDir": config.skills.user_dir,
            "externalDirs": config.skills.external_dirs,
            "disabled": config.skills.disabled,
            "autoLearnMinToolCalls": config.skills.auto_learn_min_tool_calls,
        },
        "mcpServers": mcp_servers,
        "builtinMcpServers": builtin_mcp_servers,
        "webSearch": {
            "mode": web_search_mode_label(config.web_search.mode),
            "contextSize": config
                .web_search
                .context_size
                .map(web_search_context_size_label),
            "allowedDomains": config.web_search.allowed_domains,
            "location": config.web_search.location.as_ref().map(|location| json!({
                "country": location.country,
                "region": location.region,
                "city": location.city,
                "timezone": location.timezone,
            })),
        },
    }))
}

fn model_projection(model: &ModelInfo) -> Value {
    let reasoning_efforts = model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "effort")
        .map(|parameter| parameter.candidates.as_slice())
        .unwrap_or_default();
    json!({
        "slug": model.slug,
        "displayName": model.display_name,
        "description": model.description,
        "contextWindow": model.context_window,
        "maxOutputTokens": model.max_output_tokens,
        "currency": model.currency,
        "inputPricePerMTok": model.input_price_per_mtok,
        "outputPricePerMTok": model.output_price_per_mtok,
        "cacheReadPricePerMTok": model.cache_read_price_per_mtok,
        "reasoningEfforts": reasoning_efforts,
        "baseInstructions": model.base_instructions,
    })
}

fn mcp_transport_label(transport: McpServerTransport) -> &'static str {
    match transport {
        McpServerTransport::Stdio => "stdio",
        McpServerTransport::StreamableHttp => "streamableHttp",
    }
}

fn openai_compaction_mode_label(mode: pl_studio_runtime::OpenAiCompactionMode) -> &'static str {
    match mode {
        pl_studio_runtime::OpenAiCompactionMode::RemoteV2 => "remoteV2",
        pl_studio_runtime::OpenAiCompactionMode::Local => "local",
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
        roles: input.roles.into_iter().map(role_edit).collect(),
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
    let protocol = match input.wire_protocol.trim() {
        "responses" => ProviderWireProtocol::Responses,
        "chat_completions" => ProviderWireProtocol::ChatCompletions,
        protocol => anyhow::bail!("unsupported provider wire protocol: {protocol}"),
    };
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
        }),
        source => anyhow::bail!("unsupported provider capability source: {source}"),
    };
    Ok(ProviderEdit {
        key: input.id,
        original_key: input.original_id,
        preset,
        protocol,
        connection_mode: match input.connection_mode.as_str() {
            "web_socket" => pl_studio_runtime::ProviderConnectionMode::WebSocket,
            "http" => pl_studio_runtime::ProviderConnectionMode::Http,
            mode => anyhow::bail!("unsupported provider connection mode: {mode}"),
        },
        name: input.name,
        base_url: Some(input.base_url),
        bearer_token,
        capabilities,
        default_model: input.default_model,
        custom_models: input
            .custom_models
            .into_iter()
            .map(provider_model_edit)
            .collect(),
    })
}

fn provider_model_edit(input: ProviderModelInput) -> ProviderModelEdit {
    ProviderModelEdit {
        slug: input.slug,
        display_name: input.display_name,
        efforts: input.reasoning_efforts,
        base_instructions: input.base_instructions.unwrap_or_default(),
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

fn role_edit(input: RoleInput) -> RoleEdit {
    RoleEdit {
        key: input.key,
        provider: input.provider,
        model: input.model,
        effort: input.effort,
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
                wire_protocol: "responses".to_string(),
                connection_mode: "web_socket".to_string(),
                name: "OpenAI Team".to_string(),
                base_url: "https://api.openai.com/v1".to_string(),
                secret: ProviderSecretInput::Preserve,
                capability_source: "preset_defaults".to_string(),
                hosted_web_search: true,
                standalone_web_search: Some("open_ai_search_api".to_string()),
                default_model: "gpt-5.6-sol".to_string(),
                custom_models: Vec::new(),
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
