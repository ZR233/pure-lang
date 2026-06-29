use crate::api::studio::types::{
    DeepSeekBalanceDto, DeepSeekBalanceInfoDto, ProviderInput, ProviderModelInput,
    ProviderSettingsInput, ProviderUsageDto, RoleInput, ZhipuCodingPlanUsageDto,
    ZhipuQuotaLimitDto, ZhipuToolUsageDetailDto,
};
use anyhow::{Context, Result};
use pl_core::{
    McpServerTransport, ProviderEdit, ProviderModelEdit, ProviderSettingsEdit,
    ProviderTemplateKind, ProviderUsageData, ProviderUsageState, RoleEdit, ZhipuQuotaWindow,
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

pub(crate) fn mcp_transport_from_label(label: &str) -> McpServerTransport {
    match label.trim() {
        "streamableHttp" | "streamable_http" | "http" => McpServerTransport::StreamableHttp,
        _ => McpServerTransport::Stdio,
    }
}

// ── Provider settings converters ──

pub(crate) fn provider_settings_edit(
    input: ProviderSettingsInput,
    current: &pl_core::PureConfig,
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

fn provider_edit(input: ProviderInput, current: &pl_core::PureConfig) -> Result<ProviderEdit> {
    let kind = ProviderTemplateKind::from_key(&input.template_kind).with_context(|| {
        format!(
            "unsupported provider template: {kind}",
            kind = input.template_kind
        )
    })?;
    let current_token = current
        .providers
        .get(&input.id)
        .and_then(|provider| provider.bearer_token.clone());
    let bearer_token = if input.bearer_token.trim().is_empty() {
        current_token
    } else {
        Some(input.bearer_token)
    };
    Ok(ProviderEdit {
        key: input.id,
        kind,
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

pub(crate) fn provider_usage_dto(record: pl_core::ProviderUsageRecord) -> ProviderUsageDto {
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
