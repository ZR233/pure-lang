//! Provider 设置编辑：把 wire 层的 provider/model/role 更新解析为配置编辑对象。

use anyhow::Result;
use pl_model::completion::WebSearchMode;
use pl_model::provider::{
    PromptCacheDialect, PromptCacheProviderCapabilities, ProviderConnectionMode,
    ProviderServiceCapabilities, ProviderWireProtocol, ResponsesHostedToolCapabilities,
    StandaloneWebSearchDialect, WebSearchProviderCapabilities,
};
use pl_protocol::WebSearchContextSize;
use pl_protocol::studio::{
    ProviderModelConnectionUpdate, ProviderModelUpdate, ProviderSecretUpdate,
    ProviderSettingsUpdate, RoleSettingsUpdate, StudioError, UpdateWebSearchSettingsRequest,
};

use crate::{
    ProviderCapabilitySelection, ProviderEdit, ProviderModelEdit, ProviderPresetId, RoleEdit,
};

use super::view::{normalized_optional, normalized_string_list};

pub(super) fn provider_edit(
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

pub(super) fn invalid_settings_argument(message: &'static str) -> anyhow::Error {
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

pub(super) fn web_search_config(
    request: UpdateWebSearchSettingsRequest,
) -> Result<(u64, pl_model::completion::WebSearchConfig)> {
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
        pl_model::completion::WebSearchConfig {
            mode,
            context_size,
            allowed_domains: normalized_string_list(request.allowed_domains),
            location: (!location.is_empty()).then_some(location),
        },
    ))
}
