use crate::api::studio::types::{
    DeepSeekBalanceDto, DeepSeekBalanceInfoDto, ProviderInput, ProviderModelInput,
    ProviderSettingsInput, ProviderUsageDto, RoleInput, ZhipuCodingPlanUsageDto,
    ZhipuQuotaLimitDto, ZhipuToolUsageDetailDto,
};
use anyhow::{Context, Result};
use pl_studio_runtime::{
    McpServerTransport, ProviderEdit, ProviderModelEdit, ProviderPresetId, ProviderSettingsEdit,
    ProviderUsageData, ProviderUsageState, ProviderWireProtocol, RoleEdit, ZhipuQuotaWindow,
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

pub(crate) fn mcp_transport_from_label(label: &str) -> McpServerTransport {
    match label.trim() {
        "streamableHttp" | "streamable_http" | "http" => McpServerTransport::StreamableHttp,
        _ => McpServerTransport::Stdio,
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
        providers.insert(
            id.to_string(),
            json!({
                "presetId": provider.preset_id().map(ToString::to_string),
                "templateKind": provider.preset_id().map(ToString::to_string),
                "wireProtocol": protocol_label(provider.protocol()?),
                "connectionMode": connection_mode_label(provider.connection_mode()),
                "name": provider.name,
                "baseUrl": provider.base_url,
                "hasBearerToken": provider.resolved_bearer_token().is_some(),
                "defaultModel": default_model,
                "models": models,
                "customModels": provider.editable_models(),
                "catalog": provider.catalog,
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
                    "effort": route.reasoning_effort.as_ref().map(|effort| effort.as_str()),
                }),
            )
        })
        .collect::<Map<_, _>>();
    Ok(json!({
        "schemaVersion": config.schema_version,
        "providers": providers,
        "roles": roles,
        "runtime": config.runtime,
        "instructions": config.instructions,
        "skills": config.skills,
        "mcpServers": config.mcp.servers,
        "builtinMcpServers": config.mcp.builtin_servers,
    }))
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
    let bearer_token = if input.bearer_token.trim().is_empty() {
        current_token
    } else {
        Some(input.bearer_token)
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
        reasoning_efforts: input.reasoning_efforts,
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
                bearer_token: String::new(),
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
