use crate::api::studio::types::{
    BridgeGeneralSettingsDto, BridgeInstructionsSettingsDto, BridgeMcpServerConfiguration,
    BridgeMcpServerSettingsDto, BridgeProviderModelSettingsDto, BridgeProviderSettingsDto,
    BridgeProviderUsageData, BridgeProviderUsageState, BridgeRoleSettingsDto,
    BridgeSettingsStateSnapshot, BridgeSkillsSettingsDto, BridgeStateError,
    BridgeStudioSettingsDto, BridgeWebSearchSettingsDto, DeepSeekBalanceDto,
    DeepSeekBalanceInfoDto, ProviderSecretInput, ProviderSettingsInput, ProviderUsageDto,
    ZhipuCodingPlanUsageDto, ZhipuQuotaLimitDto, ZhipuToolUsageDetailDto,
};
use pl_studio_runtime::{ProviderUsageData, ProviderUsageState, ZhipuQuotaWindow};

pub(crate) fn bridge_settings_snapshot(
    snapshot: pl_protocol::studio::StudioSettingsSnapshot,
) -> BridgeSettingsStateSnapshot {
    super::runtime::bridge_settings_state(pl_protocol::ObservedResource::ready(
        snapshot.revision,
        snapshot.updated_at,
        snapshot.settings,
    ))
}

pub(crate) fn bridge_settings(
    settings: pl_protocol::studio::StudioSettings,
) -> BridgeStudioSettingsDto {
    BridgeStudioSettingsDto {
        default_provider_id: settings.default_provider_id,
        providers: settings
            .providers
            .into_iter()
            .map(|provider| BridgeProviderSettingsDto {
                id: provider.id,
                template_kind: provider.template_kind,
                name: provider.name,
                base_url: provider.base_url,
                has_bearer_token: provider.has_bearer_token,
                capability_source: provider.capability_source,
                hosted_web_search: provider.hosted_web_search,
                standalone_web_search: provider.standalone_web_search,
                prompt_cache_dialect: provider.prompt_cache_dialect,
                responses_programmatic_tool_calling: provider.responses_programmatic_tool_calling,
                default_model: provider.default_model,
                models: provider
                    .models
                    .into_iter()
                    .map(bridge_provider_model_settings)
                    .collect(),
                custom_models: provider
                    .custom_models
                    .into_iter()
                    .map(bridge_provider_model_settings)
                    .collect(),
                catalog_id: provider.catalog_id,
            })
            .collect(),
        roles: settings
            .roles
            .into_iter()
            .map(|role| BridgeRoleSettingsDto {
                key: role.key,
                provider_id: role.provider_id,
                model: role.model,
                effort: role.effort,
            })
            .collect(),
        permission_mode: settings.permission_mode,
        instructions: BridgeInstructionsSettingsDto {
            base_override: settings.instructions.base_override,
            developer: settings.instructions.developer,
            user: settings.instructions.user,
            project_doc_max_bytes: settings.instructions.project_doc_max_bytes,
            project_doc_fallback_filenames: settings.instructions.project_doc_fallback_filenames,
        },
        skills: BridgeSkillsSettingsDto {
            enabled: settings.skills.enabled,
            auto_learn: settings.skills.auto_learn,
            system_enabled: settings.skills.system_enabled,
            project_dir: settings.skills.project_dir,
            user_dir: settings.skills.user_dir,
            external_dirs: settings.skills.external_dirs,
            disabled: settings.skills.disabled,
            auto_learn_min_tool_calls: settings.skills.auto_learn_min_tool_calls,
        },
        mcp_servers: settings
            .mcp_servers
            .into_iter()
            .map(|server| BridgeMcpServerSettingsDto {
                id: server.id,
                transport: server.transport,
                endpoint: server.endpoint,
                configuration: match server.configuration {
                    pl_protocol::studio::StudioMcpServerConfiguration::Enabled => {
                        BridgeMcpServerConfiguration::Enabled
                    }
                    pl_protocol::studio::StudioMcpServerConfiguration::Disabled => {
                        BridgeMcpServerConfiguration::Disabled
                    }
                    pl_protocol::studio::StudioMcpServerConfiguration::MissingCredential => {
                        BridgeMcpServerConfiguration::MissingCredential
                    }
                },
                source_kind: server.source_kind,
                mutation_policy: server.mutation_policy,
            })
            .collect(),
        general: BridgeGeneralSettingsDto {
            follow_system_theme: settings.general.follow_system_theme,
            follow_active_turn: settings.general.follow_active_turn,
            compact_timeline: settings.general.compact_timeline,
        },
        web_search: bridge_web_search_settings(settings.web_search),
    }
}

pub(crate) fn bridge_web_search_settings(
    settings: pl_protocol::studio::StudioWebSearchSettings,
) -> BridgeWebSearchSettingsDto {
    BridgeWebSearchSettingsDto {
        configured_mode: settings.configured_mode,
        effective_mode: settings.effective_mode,
        availability: settings.availability,
        context_size: settings.context_size,
        allowed_domains: settings.allowed_domains,
        country: settings.country,
        region: settings.region,
        city: settings.city,
        timezone: settings.timezone,
        provider_id: settings.provider_id,
        model: settings.model,
    }
}

fn bridge_provider_model_settings(
    model: pl_protocol::studio::StudioProviderModelSettings,
) -> BridgeProviderModelSettingsDto {
    BridgeProviderModelSettingsDto {
        slug: model.slug,
        display_name: model.display_name,
        description: model.description,
        context_window: model.context_window,
        max_output_tokens: model.max_output_tokens,
        currency: model.currency,
        input_price_per_m_tok: model.input_price_per_m_tok,
        output_price_per_m_tok: model.output_price_per_m_tok,
        cache_read_price_per_m_tok: model.cache_read_price_per_m_tok,
        cache_write_price_per_m_tok: model.cache_write_price_per_m_tok,
        reasoning_efforts: model.reasoning_efforts,
        base_instructions: model.base_instructions,
        wire_protocol: model.wire_protocol,
        supported_connection_modes: model.supported_connection_modes,
        default_connection_mode: model.default_connection_mode,
        connection_mode: model.connection_mode,
    }
}

pub(crate) fn provider_settings_request(
    expected_revision: u64,
    input: ProviderSettingsInput,
) -> pl_protocol::studio::UpdateProviderSettingsRequest {
    pl_protocol::studio::UpdateProviderSettingsRequest {
        expected_revision,
        default_provider_id: input.default_provider_id,
        providers: input
            .providers
            .into_iter()
            .map(|provider| pl_protocol::studio::ProviderSettingsUpdate {
                id: provider.id,
                original_id: provider.original_id,
                template_kind: provider.template_kind,
                name: provider.name,
                base_url: provider.base_url,
                secret: match provider.secret {
                    ProviderSecretInput::Preserve => {
                        pl_protocol::studio::ProviderSecretUpdate::Preserve
                    }
                    ProviderSecretInput::Replace { value } => {
                        pl_protocol::studio::ProviderSecretUpdate::Replace { value }
                    }
                    ProviderSecretInput::Clear => pl_protocol::studio::ProviderSecretUpdate::Clear,
                },
                capability_source: provider.capability_source,
                hosted_web_search: provider.hosted_web_search,
                standalone_web_search: provider.standalone_web_search,
                prompt_cache_dialect: provider.prompt_cache_dialect,
                responses_programmatic_tool_calling: provider.responses_programmatic_tool_calling,
                default_model: provider.default_model,
                custom_models: provider
                    .custom_models
                    .into_iter()
                    .map(|model| pl_protocol::studio::ProviderModelUpdate {
                        slug: model.slug,
                        display_name: model.display_name,
                        reasoning_efforts: model.reasoning_efforts,
                        base_instructions: model.base_instructions,
                        wire_protocol: model.wire_protocol,
                        supported_connection_modes: model.supported_connection_modes,
                        default_connection_mode: model.default_connection_mode,
                    })
                    .collect(),
                model_connection_modes: provider
                    .model_connection_modes
                    .into_iter()
                    .map(|mode| pl_protocol::studio::ProviderModelConnectionUpdate {
                        slug: mode.slug,
                        connection_mode: mode.connection_mode,
                    })
                    .collect(),
            })
            .collect(),
        roles: input
            .roles
            .into_iter()
            .map(|role| pl_protocol::studio::RoleSettingsUpdate {
                key: role.key,
                provider: role.provider,
                model: role.model,
                effort: role.effort,
            })
            .collect(),
    }
}
pub(crate) fn provider_usage_dto(
    record: pl_studio_runtime::ProviderUsageRecord,
) -> ProviderUsageDto {
    let state = match record.state() {
        ProviderUsageState::Unsupported(_) => BridgeProviderUsageState::Unsupported,
        ProviderUsageState::MissingCredential(state) => {
            BridgeProviderUsageState::MissingCredential {
                message: state.message().to_string(),
            }
        }
        ProviderUsageState::Failed(state) => BridgeProviderUsageState::Failed {
            error: BridgeStateError {
                code: state.error().code.clone(),
                message: state.error().message.clone(),
                retryable: state.error().retryable,
            },
        },
        ProviderUsageState::Ready(state) => match state.data() {
            ProviderUsageData::DeepSeekBalance(balance) => BridgeProviderUsageState::Ready {
                data: BridgeProviderUsageData::DeepSeekBalance(DeepSeekBalanceDto {
                    is_available: balance.is_available,
                    balances: balance
                        .balances
                        .iter()
                        .map(|item| DeepSeekBalanceInfoDto {
                            currency: item.currency.clone(),
                            total_balance: item.total_balance.clone(),
                            granted_balance: item.granted_balance.clone(),
                            topped_up_balance: item.topped_up_balance.clone(),
                        })
                        .collect(),
                }),
            },
            ProviderUsageData::ZhipuCodingPlan(usage) => BridgeProviderUsageState::Ready {
                data: BridgeProviderUsageData::ZhipuCodingPlan(ZhipuCodingPlanUsageDto {
                    level: usage.level.clone(),
                    limits: usage
                        .limits
                        .iter()
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
                                    .iter()
                                    .map(|detail| ZhipuToolUsageDetailDto {
                                        name: detail.name.clone(),
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
        },
    };
    ProviderUsageDto {
        provider_id: record.provider_id().to_string(),
        revision: record.revision(),
        updated_at: record.updated_at(),
        state,
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
