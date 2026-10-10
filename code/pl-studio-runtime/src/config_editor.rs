use std::collections::BTreeMap;
use std::collections::BTreeSet;

use crate::{PureError, Result};

use crate::config::{ModelRouteConfig, ProviderId, ReasoningEffort, StudioConfig};
use crate::first_run::ProviderTemplateKind;
use pl_model::config::{
    ProviderConfig, ProviderModelCatalogConfig, ProviderPresetId, builtin_provider_catalog,
};
use pl_model::model::{ModelInfo, ModelTransportProfile};
use pl_model::provider::{ProviderConnectionMode, ProviderEndpoint, ProviderWireProtocol};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderModelEdit {
    pub slug: String,
    pub display_name: String,
    pub protocol: ProviderWireProtocol,
    pub context_window: u64,
    pub max_output_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderEdit {
    pub key: String,
    pub original_key: Option<String>,
    pub preset: Option<ProviderPresetId>,
    pub name: String,
    pub base_url: Option<String>,
    pub bearer_token: Option<String>,
    pub pricing_mode: pl_protocol::PricingMode,
    pub default_model: String,
    pub custom_models: Vec<ProviderModelEdit>,
    pub model_connection_modes: BTreeMap<String, ProviderConnectionMode>,
    /// 该 provider 实例完整的上下文压缩阈值用户覆盖集合；未列出模型使用默认值。
    pub model_auto_compact_limits: BTreeMap<String, u64>,
}

/// # Errors
/// Returns the bundled catalog assembly error without treating it as a missing preset.
pub fn provider_template_kind(provider: &ProviderConfig) -> Result<Option<ProviderTemplateKind>> {
    provider
        .preset_id()
        .map(|preset| ProviderTemplateKind::from_key(preset.as_str()))
        .transpose()
        .map(Option::flatten)
}

impl ProviderModelEdit {
    fn to_model_info(&self, current: Option<&ModelInfo>) -> Result<ModelInfo> {
        let slug = non_empty_trimmed(&self.slug, "model slug")?;
        let mut model = current
            .filter(|model| model.slug == slug)
            .cloned()
            .unwrap_or_else(|| ModelInfo::compatible(&slug));
        if let Some(current) = current
            && current.slug == slug
            && current.display_name
                == trim_optional(Some(&self.display_name)).unwrap_or_else(|| slug.clone())
            && current.context_window.unwrap_or(32_000) == self.context_window
            && current.max_output_tokens.unwrap_or(4_096) == self.max_output_tokens
            && current.binding.transport.protocol == self.protocol
        {
            return Ok(current.clone());
        }
        model.display_name =
            trim_optional(Some(&self.display_name)).unwrap_or_else(|| slug.clone());
        if self.context_window == 0
            || self.max_output_tokens == 0
            || self.max_output_tokens > self.context_window
        {
            return Err(PureError::ConfigError(
                "custom model budgets must be positive and output must fit its context".into(),
            ));
        }
        model.context_window = Some(self.context_window);
        model.max_context_window = Some(self.context_window);
        model.max_output_tokens = Some(self.max_output_tokens);
        model.binding.request.protocol = match self.protocol {
            ProviderWireProtocol::Responses => {
                pl_model::model::ModelProtocolOptions::Responses(Default::default())
            }
            ProviderWireProtocol::ChatCompletions => {
                pl_model::model::ModelProtocolOptions::ChatCompletions(
                    pl_model::model::ChatRequestOptions {
                        include_usage: true,
                        ..Default::default()
                    },
                )
            }
        };
        model.binding.transport = ModelTransportProfile {
            protocol: self.protocol,
            supported_connection_modes: vec![ProviderConnectionMode::Http],
            default_connection_mode: ProviderConnectionMode::Http,
        };
        model
            .binding
            .transport
            .validate(&slug)
            .map_err(|error| PureError::ConfigError(error.to_string()))?;
        Ok(model)
    }
}

impl ProviderEdit {
    fn provider_key(&self) -> Result<String> {
        validate_provider_key(&self.key)
    }

    fn to_provider_config(&self, current: Option<&ProviderConfig>) -> Result<EditedProvider> {
        let provider_key = self.provider_key()?;
        let name = non_empty_trimmed(&self.name, "provider name")?;
        let base_url = trim_optional(self.base_url.as_deref()).ok_or_else(|| {
            PureError::ConfigError("provider base_url must not be empty".to_string())
        })?;
        // The display default can be empty after a successful empty observation.
        // Route construction validates it only when a new selection needs it.
        let default_model = self.default_model.trim().to_string();
        let bearer_token = trim_optional(self.bearer_token.as_deref());
        let current_models = current
            .map(ProviderConfig::editable_models)
            .unwrap_or_default();
        let custom_models = self
            .custom_models
            .iter()
            .map(|edit| {
                edit.to_model_info(
                    current_models
                        .iter()
                        .find(|model| model.slug.trim() == edit.slug.trim()),
                )
            })
            .collect::<Result<Vec<_>>>()?;
        let mut config = match &self.preset {
            Some(preset_id) => {
                let preset = builtin_provider_catalog()?
                    .presets
                    .into_iter()
                    .find(|preset| &preset.id == preset_id)
                    .ok_or_else(|| {
                        PureError::ConfigError(format!(
                            "provider {provider_key} references unknown preset: {preset_id}"
                        ))
                    })?;
                let mut config = current
                    .filter(|current| current.preset_id() == Some(preset_id))
                    .cloned()
                    .unwrap_or(preset.provider);
                if current.and_then(ProviderConfig::preset_id) == Some(preset_id) {
                    let current = current.expect("matching current preset is present");
                    config.bearer_token_env = current.bearer_token_env.clone();
                    config.http_headers = current.http_headers.clone();
                    config.tool_wire_policy = current.tool_wire_policy;
                    config.apply_patch_tool_type = current.apply_patch_tool_type;
                    config.capabilities = current.capabilities.clone();
                }
                match &mut config.catalog {
                    ProviderModelCatalogConfig::Bundled {
                        additional_models, ..
                    } => *additional_models = custom_models,
                    ProviderModelCatalogConfig::Explicit { models, .. } => *models = custom_models,
                }
                config
            }
            None => {
                let current_custom = current.filter(|provider| provider.preset_id().is_none());
                let info = ProviderEndpoint {
                    adapter: pl_model::provider::ProviderAdapterKind::OpenAiCompatible,
                    name: name.clone(),
                    base_url: base_url.clone(),
                    bearer_token: bearer_token.clone(),
                    http_headers: current_custom.and_then(|provider| provider.http_headers.clone()),
                    tool_wire_policy: current_custom
                        .map(|provider| provider.tool_wire_policy)
                        .unwrap_or_default(),
                    apply_patch_tool_type: current_custom
                        .and_then(|provider| provider.apply_patch_tool_type),
                    service_capabilities: current_custom
                        .and_then(|provider| provider.service_capabilities().ok())
                        .unwrap_or_default(),
                };
                let mut config = ProviderConfig::from_explicit_models(info, custom_models);
                config.bearer_token_env =
                    current_custom.and_then(|provider| provider.bearer_token_env.clone());
                config
            }
        };
        match &mut config.catalog {
            ProviderModelCatalogConfig::Bundled {
                connection_overrides,
                auto_compact_overrides,
                ..
            }
            | ProviderModelCatalogConfig::Explicit {
                connection_overrides,
                auto_compact_overrides,
                ..
            } => {
                *connection_overrides = self.model_connection_modes.clone();
                *auto_compact_overrides = self.model_auto_compact_limits.clone();
            }
        }
        config.pricing_mode = self.pricing_mode;
        config.name = name;
        config.base_url = base_url;
        config.bearer_token = bearer_token;
        let models = config.effective_models()?;
        if !config.supports_model_discovery() {
            validate_models(&provider_key, &default_model, &models)?;
        }

        Ok(EditedProvider {
            id: ProviderId::new(provider_key)?,
            config,
        })
    }

    /// Applies one provider edit while preserving every other settings domain.
    ///
    /// The old aggregate editor rebuilt providers, mode routes and role routes
    /// from one page snapshot.  That made a stale settings page capable of
    /// reverting an unrelated route.  Single provider commands are deliberately
    /// constrained to an existing id or a new id; identity changes have their
    /// own explicit removal/rename workflow and cannot silently rewrite routes.
    pub fn to_single_config(&self, current: &StudioConfig) -> Result<StudioConfig> {
        let provider_key = self.provider_key()?;
        let provider_id = ProviderId::new(provider_key.clone())?;
        let original_id = self
            .original_key
            .as_deref()
            .map(ProviderId::new)
            .transpose()?
            .unwrap_or_else(|| provider_id.clone());
        if self.original_key.is_some() && original_id != provider_id {
            return Err(PureError::ConfigError(
                "provider identity changes require an explicit rename operation".into(),
            ));
        }
        if self.original_key.is_none() && current.models.providers.contains_key(&provider_id) {
            return Err(PureError::ConfigError(format!(
                "provider already exists: {provider_id}"
            )));
        }
        let edited = self.to_provider_config(current.models.providers.get(&original_id))?;
        let mut next = current.clone();
        next.models.providers.insert(edited.id, edited.config);
        next.validate_declarations()?;
        Ok(next)
    }
}

/// Removes one provider.  Route migration is only performed when the caller
/// explicitly supplies a replacement provider; an old page snapshot can never
/// cause an implicit route rewrite.
pub fn remove_provider(
    current: &StudioConfig,
    provider_key: &str,
    replacement_key: Option<&str>,
) -> Result<StudioConfig> {
    if current.models.providers.len() <= 1 {
        return Err(PureError::ConfigError(
            "at least one provider is required".into(),
        ));
    }
    let provider_id = ProviderId::new(provider_key.trim())?;
    if !current.models.providers.contains_key(&provider_id) {
        return Err(PureError::ConfigError(format!(
            "provider does not exist: {provider_id}"
        )));
    }
    let replacement_id = replacement_key
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ProviderId::new)
        .transpose()?;
    if replacement_id.as_ref() == Some(&provider_id) {
        return Err(PureError::ConfigError(
            "provider replacement must differ from the removed provider".into(),
        ));
    }
    let replacement = replacement_id
        .as_ref()
        .map(|id| {
            current
                .models
                .providers
                .get(id)
                .ok_or_else(|| PureError::ConfigError(format!("provider does not exist: {id}")))
        })
        .transpose()?;
    let mut next = current.clone();
    next.models.providers.remove(&provider_id);

    let migrate_route = |route: &mut ModelRouteConfig| -> Result<()> {
        if route.provider != provider_id {
            return Ok(());
        }
        let replacement_id = replacement_id.as_ref().ok_or_else(|| {
            PureError::ConfigError(format!(
                "provider {provider_id} is still referenced by a route; specify replacement_provider_id"
            ))
        })?;
        let replacement = replacement.expect("validated replacement provider");
        let models = replacement.effective_models()?;
        let model = models
            .iter()
            .find(|model| model.slug == route.model)
            .or_else(|| models.first())
            .ok_or_else(|| {
                PureError::ConfigError(format!(
                    "replacement provider has no usable models: {replacement_id}"
                ))
            })?;
        let effort = route
            .effort
            .as_ref()
            .filter(|effort| {
                model
                    .supported_efforts()
                    .iter()
                    .any(|value| value == effort.as_str())
            })
            .cloned()
            .or_else(|| model.default_effort().map(ReasoningEffort::new));
        route.provider = replacement_id.clone();
        route.model = model.slug.clone();
        route.effort = effort;
        Ok(())
    };
    for route in next.models.routes.values_mut() {
        migrate_route(route)?;
    }
    for route in next.mode_model_routes.values_mut() {
        migrate_route(route)?;
    }
    next.validate_declarations()?;
    Ok(next)
}

struct EditedProvider {
    id: ProviderId,
    config: ProviderConfig,
}

fn non_empty_trimmed(value: &str, name: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(PureError::ConfigError(format!("{name} must not be empty")));
    }
    Ok(trimmed.to_string())
}

fn trim_optional(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn validate_provider_key(key: &str) -> Result<String> {
    let key = non_empty_trimmed(key, "provider key")?;
    if !key
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    {
        return Err(PureError::ConfigError(format!(
            "provider key contains unsupported characters: {key}"
        )));
    }
    Ok(key)
}

fn validate_models(provider_key: &str, default_model: &str, models: &[ModelInfo]) -> Result<()> {
    let mut slugs = BTreeSet::new();
    for model in models {
        if model.slug.trim().is_empty() {
            return Err(PureError::ConfigError(format!(
                "provider {provider_key} has a model with empty slug"
            )));
        }
        if !slugs.insert(model.slug.clone()) {
            return Err(PureError::ConfigError(format!(
                "provider {provider_key} has duplicate model slug: {}",
                model.slug
            )));
        }
    }

    let _model = models
        .iter()
        .find(|model| model.slug == default_model)
        .ok_or_else(|| {
            PureError::ConfigError(format!(
                "provider {provider_key} default_model is not in models: {default_model}"
            ))
        })?;
    Ok(())
}
