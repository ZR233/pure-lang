use std::collections::BTreeMap;
use std::collections::BTreeSet;

use crate::{PureError, Result};
use pl_model::{
    ModelInfo, ModelParameter, ModelTransportProfile, ProviderConnectionMode, ProviderEndpoint,
    ProviderWireProtocol,
};

use crate::config::{
    ModelRouteConfig, ProviderId, ReasoningEffort, STUDIO_CONFIG_SCHEMA_VERSION, StudioConfig,
    StudioRole,
};
use crate::first_run::ProviderTemplateKind;
use crate::{
    AgentModelConfig, ProviderCapabilitySelection, ProviderConfig, ProviderModelCatalogConfig,
    ProviderPresetId, builtin_provider_catalog,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderModelEdit {
    pub slug: String,
    pub display_name: String,
    pub efforts: Vec<String>,
    pub base_instructions: String,
    pub protocol: ProviderWireProtocol,
    pub supported_connection_modes: Vec<ProviderConnectionMode>,
    pub default_connection_mode: ProviderConnectionMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderEdit {
    pub key: String,
    pub original_key: Option<String>,
    pub preset: Option<ProviderPresetId>,
    pub name: String,
    pub base_url: Option<String>,
    pub bearer_token: Option<String>,
    pub capabilities: ProviderCapabilitySelection,
    pub default_model: String,
    pub custom_models: Vec<ProviderModelEdit>,
    pub model_connection_modes: BTreeMap<String, ProviderConnectionMode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSettingsEdit {
    pub default_provider: Option<String>,
    pub providers: Vec<ProviderEdit>,
    pub roles: Vec<RoleEdit>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleEdit {
    pub key: String,
    pub provider: String,
    pub model: String,
    pub effort: String,
}

pub fn provider_template_kind(provider: &ProviderConfig) -> Option<ProviderTemplateKind> {
    provider
        .preset_id()
        .and_then(|preset| ProviderTemplateKind::from_key(preset.as_str()))
}

impl ProviderModelEdit {
    fn to_model_info(&self, current: Option<&ModelInfo>) -> Result<ModelInfo> {
        let slug = non_empty_trimmed(&self.slug, "model slug")?;
        let mut model = current
            .filter(|model| model.slug == slug)
            .cloned()
            .unwrap_or_else(|| ModelInfo::fallback(&slug));
        model.display_name = non_empty_trimmed(&self.display_name, "model display_name")?;
        let efforts = normalized_efforts(&self.efforts);
        let existing_effort = model
            .parameters
            .iter()
            .find(|parameter| parameter.name == "effort");
        let effort_wire = existing_effort
            .filter(|parameter| parameter.candidates == efforts)
            .map(|parameter| parameter.wire.clone())
            .unwrap_or_default();
        let effort_label = existing_effort.and_then(|parameter| parameter.label.clone());
        model
            .parameters
            .retain(|parameter| parameter.name != "effort");
        if !efforts.is_empty() {
            model.parameters.push(ModelParameter {
                name: "effort".to_string(),
                label: effort_label,
                candidates: efforts,
                wire: effort_wire,
            });
        }
        model.base_instructions = self.base_instructions.trim().to_string();
        model.transport = ModelTransportProfile {
            protocol: self.protocol,
            supported_connection_modes: self.supported_connection_modes.clone(),
            default_connection_mode: self.default_connection_mode,
        };
        model
            .transport
            .validate(&slug)
            .map_err(PureError::ConfigError)?;
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
        let default_model = non_empty_trimmed(&self.default_model, "provider default_model")?;
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
                let preset = builtin_provider_catalog()
                    .presets
                    .into_iter()
                    .find(|preset| &preset.id == preset_id)
                    .ok_or_else(|| {
                        PureError::ConfigError(format!(
                            "provider {provider_key} references unknown preset: {preset_id}"
                        ))
                    })?;
                let mut config = preset.provider;
                if current.and_then(ProviderConfig::preset_id) == Some(preset_id) {
                    let current = current.expect("matching current preset is present");
                    config.bearer_token_env = current.bearer_token_env.clone();
                    config.http_headers = current.http_headers.clone();
                    config.tool_wire_policy = current.tool_wire_policy;
                    config.apply_patch_tool_type = current.apply_patch_tool_type;
                    config.capabilities = current.capabilities.clone();
                }
                let ProviderModelCatalogConfig::Bundled {
                    additional_models, ..
                } = &mut config.catalog
                else {
                    return Err(PureError::ConfigError(format!(
                        "provider preset {preset_id} must use a bundled catalog"
                    )));
                };
                *additional_models = custom_models;
                config
            }
            None => {
                if matches!(
                    self.capabilities,
                    ProviderCapabilitySelection::PresetDefaults
                ) {
                    return Err(PureError::ConfigError(format!(
                        "custom provider {provider_key} must use explicit service capabilities"
                    )));
                }
                let current_custom = current.filter(|provider| provider.preset_id().is_none());
                let info = ProviderEndpoint {
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
        for (model, mode) in &self.model_connection_modes {
            config.set_model_connection_mode(model, *mode)?;
        }
        config.capabilities = self.capabilities.clone();
        config.name = name;
        config.base_url = base_url;
        config.bearer_token = bearer_token;
        let models = config.effective_models()?;
        validate_models(&provider_key, &default_model, &models)?;

        Ok(EditedProvider {
            id: ProviderId::new(provider_key)?,
            default_model,
            config,
        })
    }
}

struct EditedProvider {
    id: ProviderId,
    config: ProviderConfig,
    default_model: String,
}

impl ProviderSettingsEdit {
    pub fn to_config(&self, current: &StudioConfig) -> Result<StudioConfig> {
        if self.providers.is_empty() {
            return Err(PureError::ConfigError(
                "at least one provider is required".to_string(),
            ));
        }

        let mut provider_keys = BTreeSet::new();
        let mut providers = BTreeMap::new();
        let mut default_models = BTreeMap::new();
        for provider in &self.providers {
            let provider_key = provider.provider_key()?;
            if !provider_keys.insert(provider_key.clone()) {
                return Err(PureError::ConfigError(format!(
                    "duplicate provider key: {provider_key}"
                )));
            }
            let provider_id = ProviderId::new(provider_key)?;
            let current_provider_id = provider
                .original_key
                .as_deref()
                .map(ProviderId::new)
                .transpose()?
                .unwrap_or_else(|| provider_id.clone());
            let edited =
                provider.to_provider_config(current.models.providers.get(&current_provider_id))?;
            default_models.insert(edited.id.clone(), edited.default_model);
            providers.insert(edited.id, edited.config);
        }

        let fallback_provider = self
            .default_provider
            .as_deref()
            .map(str::trim)
            .and_then(|key| ProviderId::new(key).ok())
            .filter(|key| providers.contains_key(key))
            .or_else(|| {
                current
                    .models
                    .routes
                    .get(&StudioRole::Planner.id())
                    .map(|route| route.provider.clone())
                    .filter(|provider| providers.contains_key(provider))
            })
            .or_else(|| providers.keys().next().cloned())
            .ok_or_else(|| {
                PureError::ConfigError("at least one provider is required".to_string())
            })?;

        let routes = if self.roles.is_empty() {
            StudioRole::all()
                .into_iter()
                .map(|role| {
                    let route = current.models.routes.get(&role.id());
                    Ok((
                        role.id(),
                        reconciled_route(route, &providers, &default_models, &fallback_provider)?,
                    ))
                })
                .collect::<Result<BTreeMap<_, _>>>()?
        } else {
            role_edits_to_routes(&self.roles, &providers, &default_models)?
        };
        let mut config = StudioConfig {
            schema_version: STUDIO_CONFIG_SCHEMA_VERSION,
            models: AgentModelConfig { providers, routes },
            disabled_system_agents: current.disabled_system_agents.clone(),
            web_search: current.web_search.clone(),
            runtime: current.runtime.clone(),
            instructions: current.instructions.clone(),
            skills: current.skills.clone(),
            mcp: current.mcp.clone(),
            lsp: current.lsp.clone(),
            ui: current.ui.clone(),
        };
        crate::config::normalize_builtin_mcp_server_states(&mut config);
        config.validate()?;
        Ok(config)
    }
}

fn role_edits_to_routes(
    edits: &[RoleEdit],
    providers: &BTreeMap<ProviderId, ProviderConfig>,
    default_models: &BTreeMap<ProviderId, String>,
) -> Result<BTreeMap<crate::AgentRoleId, ModelRouteConfig>> {
    let fallback_provider = providers
        .keys()
        .next()
        .ok_or_else(|| PureError::ConfigError("at least one provider is required".to_string()))?
        .clone();
    let fallback_route = route_for_provider_default(providers, default_models, &fallback_provider)?;
    let mut routes = StudioRole::all()
        .into_iter()
        .map(|role| (role.id(), fallback_route.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut seen = BTreeSet::new();

    for edit in edits {
        let role = StudioRole::from_key(edit.key.trim()).ok_or_else(|| {
            let key = &edit.key;
            PureError::ConfigError(format!("unsupported model role: {key}"))
        })?;
        if !seen.insert(role) {
            return Err(PureError::ConfigError(format!(
                "duplicate model role: {}",
                role.key()
            )));
        }
        routes.insert(role.id(), role_edit_to_route(edit, providers, role)?);
    }

    Ok(routes)
}

fn role_edit_to_route(
    edit: &RoleEdit,
    providers: &BTreeMap<ProviderId, ProviderConfig>,
    role: StudioRole,
) -> Result<ModelRouteConfig> {
    let provider_key = non_empty_trimmed(&edit.provider, "role provider")?;
    let provider_id = ProviderId::new(provider_key.clone())?;
    let provider = providers.get(&provider_id).ok_or_else(|| {
        let role_key = role.key();
        PureError::ConfigError(format!(
            "role {role_key} references missing provider: {provider_key}"
        ))
    })?;
    let model_slug = non_empty_trimmed(&edit.model, "role model")?;
    let models = provider.effective_models()?;
    let model = models
        .iter()
        .find(|model| model.slug == model_slug)
        .ok_or_else(|| {
            let role_key = role.key();
            PureError::ConfigError(format!(
                "role {role_key} references missing model: {provider_key}.{model_slug}"
            ))
        })?;
    let effort = edit.effort.trim();
    let effort = if effort.is_empty() {
        model.default_effort().ok_or_else(|| {
            let role_key = role.key();
            PureError::ConfigError(format!("role {role_key} model must define effort"))
        })?
    } else if model
        .supported_efforts()
        .iter()
        .any(|candidate| candidate == effort)
    {
        effort.to_string()
    } else {
        return Err(PureError::ConfigError(format!(
            "role {} uses unsupported effort '{}' for model {provider_key}.{model_slug}",
            role.key(),
            effort
        )));
    };

    Ok(ModelRouteConfig {
        provider: provider_id,
        model: model_slug,
        effort: Some(ReasoningEffort::new(effort)),
    })
}

fn reconciled_route(
    route: Option<&ModelRouteConfig>,
    providers: &BTreeMap<ProviderId, ProviderConfig>,
    default_models: &BTreeMap<ProviderId, String>,
    fallback_provider: &ProviderId,
) -> Result<ModelRouteConfig> {
    if let Some(route) = route
        && let Some(provider) = providers.get(&route.provider)
        && let Ok(models) = provider.effective_models()
        && let Some(model) = models.iter().find(|model| model.slug == route.model)
    {
        if route.effort.as_ref().is_none_or(|configured| {
            model
                .supported_efforts()
                .iter()
                .any(|effort| effort == configured.as_str())
        }) {
            return Ok(route.clone());
        }
        if let Some(effort) = model.default_effort() {
            return Ok(ModelRouteConfig {
                provider: route.provider.clone(),
                model: route.model.clone(),
                effort: Some(ReasoningEffort::new(effort)),
            });
        }
    }

    route_for_provider_default(providers, default_models, fallback_provider)
}

fn route_for_provider_default(
    providers: &BTreeMap<ProviderId, ProviderConfig>,
    default_models: &BTreeMap<ProviderId, String>,
    provider_key: &ProviderId,
) -> Result<ModelRouteConfig> {
    let provider = providers.get(provider_key).ok_or_else(|| {
        PureError::ConfigError(format!(
            "default provider references missing provider: {provider_key}"
        ))
    })?;
    let default_model = default_models.get(provider_key).ok_or_else(|| {
        PureError::ConfigError(format!(
            "default model is missing for provider: {provider_key}"
        ))
    })?;
    let models = provider.effective_models()?;
    let model = models
        .iter()
        .find(|model| model.slug == *default_model)
        .ok_or_else(|| {
            PureError::ConfigError(format!(
                "default model is missing from provider: {}",
                default_model
            ))
        })?;
    let effort = model.default_effort().ok_or_else(|| {
        PureError::ConfigError(format!(
            "default model {} must define effort parameter",
            default_model
        ))
    })?;

    Ok(ModelRouteConfig {
        provider: provider_key.clone(),
        model: default_model.clone(),
        effort: Some(ReasoningEffort::new(effort)),
    })
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

    let model = models
        .iter()
        .find(|model| model.slug == default_model)
        .ok_or_else(|| {
            PureError::ConfigError(format!(
                "provider {provider_key} default_model is not in models: {default_model}"
            ))
        })?;
    if model.default_effort().is_none() {
        return Err(PureError::ConfigError(format!(
            "provider {provider_key} default model {default_model} must define effort parameter"
        )));
    }
    Ok(())
}

fn normalized_efforts(values: &[String]) -> Vec<String> {
    let efforts = values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if efforts.is_empty() {
        vec!["high".to_string()]
    } else {
        efforts
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn openai_edit() -> ProviderEdit {
        ProviderEdit {
            key: "openai".to_string(),
            original_key: None,
            preset: Some(ProviderPresetId::new("openai").unwrap()),
            name: "OpenAI".to_string(),
            base_url: Some("https://api.openai.com/v1".to_string()),
            bearer_token: None,
            capabilities: ProviderCapabilitySelection::PresetDefaults,
            default_model: "gpt-5.6-sol".to_string(),
            custom_models: Vec::new(),
            model_connection_modes: BTreeMap::new(),
        }
    }

    fn route(config: &StudioConfig, role: StudioRole) -> &ModelRouteConfig {
        config.models.routes.get(&role.id()).unwrap()
    }

    #[test]
    fn removing_provider_repoints_every_studio_role() {
        let config = ProviderSettingsEdit {
            default_provider: Some("openai".to_string()),
            providers: vec![openai_edit()],
            roles: Vec::new(),
        }
        .to_config(&StudioConfig::default_config())
        .unwrap();

        for role in StudioRole::all() {
            assert_eq!(route(&config, role).provider.as_str(), "openai");
            assert_eq!(route(&config, role).model, "gpt-5.6-sol");
        }
    }
}
