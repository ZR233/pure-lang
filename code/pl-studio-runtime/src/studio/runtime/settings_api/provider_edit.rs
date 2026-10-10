//! Provider 设置编辑：把 wire 层的 provider/model/role 更新解析为配置编辑对象。

use anyhow::Result;
use pl_model::provider::{ProviderConnectionMode, ProviderWireProtocol};
use pl_protocol::studio::{
    ProviderModelAutoCompactUpdate, ProviderModelConnectionUpdate, ProviderModelUpdate,
    ProviderSecretUpdate, ProviderSettingsUpdate, StudioError,
};

use crate::{ProviderEdit, ProviderModelEdit};
use pl_model::config::ProviderPresetId;

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
        .get(&pl_model::config::ProviderId::new(current_id)?)
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
    Ok(ProviderEdit {
        key: input.id,
        original_key: input.original_id,
        preset,
        name: input.name,
        base_url: Some(input.base_url),
        bearer_token,
        pricing_mode: if input.pricing_enabled {
            pl_protocol::PricingMode::Catalog
        } else {
            pl_protocol::PricingMode::Disabled
        },
        default_model: input.default_model,
        custom_models: input
            .custom_models
            .into_iter()
            .map(provider_model_edit)
            .collect::<Result<Vec<_>>>()?,
        model_connection_modes: model_connection_modes(input.model_connection_modes)?,
        model_auto_compact_limits: model_auto_compact_limits(input.model_auto_compact_limits)?,
    })
}

fn provider_model_edit(input: ProviderModelUpdate) -> Result<ProviderModelEdit> {
    Ok(ProviderModelEdit {
        slug: input.slug,
        display_name: input.display_name,
        protocol: parse_provider_protocol(&input.wire_protocol)?,
        context_window: input.context_window,
        max_output_tokens: input.max_output_tokens,
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

fn model_auto_compact_limits(
    inputs: Vec<ProviderModelAutoCompactUpdate>,
) -> Result<std::collections::BTreeMap<String, u64>> {
    let mut limits = std::collections::BTreeMap::new();
    for input in inputs {
        let slug = input.slug.trim();
        if slug.is_empty() {
            return Err(invalid_settings_argument(
                "Model auto compact slug must not be empty",
            ));
        }
        if input.limit == 0 {
            return Err(invalid_settings_argument(
                "Model auto compact limit must be a positive integer",
            ));
        }
        if limits.insert(slug.to_string(), input.limit).is_some() {
            return Err(invalid_settings_argument(
                "Duplicate model auto compact limit",
            ));
        }
    }
    Ok(limits)
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
