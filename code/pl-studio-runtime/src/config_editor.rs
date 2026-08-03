use std::collections::BTreeMap;
use std::collections::BTreeSet;

use crate::{PureError, Result};
use pl_model::{
    ModelInfo, ModelParameter, ProviderConnectionMode, ProviderInfo, ProviderWireProtocol,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderEdit {
    pub key: String,
    pub original_key: Option<String>,
    pub preset: Option<ProviderPresetId>,
    pub protocol: ProviderWireProtocol,
    pub connection_mode: ProviderConnectionMode,
    pub name: String,
    pub base_url: Option<String>,
    pub bearer_token: Option<String>,
    pub capabilities: ProviderCapabilitySelection,
    pub default_model: String,
    pub custom_models: Vec<ProviderModelEdit>,
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
                if preset.protocol != self.protocol {
                    return Err(PureError::ConfigError(format!(
                        "provider {provider_key} preset {preset_id} requires protocol {:?}",
                        preset.protocol
                    )));
                }
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
                let current_custom = current.filter(|provider| {
                    provider.preset_id().is_none()
                        && provider.protocol().ok() == Some(self.protocol)
                });
                let info = ProviderInfo {
                    protocol: self.protocol,
                    connection_mode: self.connection_mode,
                    name: name.clone(),
                    base_url: base_url.clone(),
                    default_model: String::new(),
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
                let mut config = ProviderConfig::from_provider_info(info, custom_models);
                config.bearer_token_env =
                    current_custom.and_then(|provider| provider.bearer_token_env.clone());
                config
            }
        };
        config.set_connection_mode(self.connection_mode);
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
            web_search: current.web_search.clone(),
            runtime: current.runtime.clone(),
            instructions: current.instructions.clone(),
            skills: current.skills.clone(),
            mcp: current.mcp.clone(),
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
    use crate::ProviderServiceCapabilities;

    fn template(id: &str) -> ProviderTemplateKind {
        ProviderTemplateKind::from_key(id).unwrap()
    }

    fn openai_edit() -> ProviderEdit {
        ProviderEdit {
            key: "openai".to_string(),
            original_key: None,
            preset: Some(ProviderPresetId::new("openai").unwrap()),
            protocol: ProviderWireProtocol::Responses,
            connection_mode: ProviderConnectionMode::WebSocket,
            name: "OpenAI".to_string(),
            base_url: Some("https://api.openai.com/v1".to_string()),
            bearer_token: None,
            capabilities: ProviderCapabilitySelection::PresetDefaults,
            default_model: "gpt-5.6-sol".to_string(),
            custom_models: Vec::new(),
        }
    }

    fn zhipu_edit() -> ProviderEdit {
        ProviderEdit {
            key: "zhipu".to_string(),
            original_key: None,
            preset: Some(ProviderPresetId::new("zhipu").unwrap()),
            protocol: ProviderWireProtocol::ChatCompletions,
            connection_mode: ProviderConnectionMode::Http,
            name: "Zhipu".to_string(),
            base_url: Some("https://open.bigmodel.cn/api/paas/v4".to_string()),
            bearer_token: None,
            capabilities: ProviderCapabilitySelection::PresetDefaults,
            default_model: "glm-5.2".to_string(),
            custom_models: Vec::new(),
        }
    }

    fn provider<'a>(config: &'a StudioConfig, id: &str) -> &'a ProviderConfig {
        config
            .models
            .providers
            .get(&ProviderId::new(id).unwrap())
            .unwrap()
    }

    fn route(config: &StudioConfig, role: StudioRole) -> &ModelRouteConfig {
        config.models.routes.get(&role.id()).unwrap()
    }

    #[test]
    fn provider_edit_appends_custom_models_after_template_defaults() {
        let mut edit = openai_edit();
        edit.custom_models.push(ProviderModelEdit {
            slug: "gpt-custom".to_string(),
            display_name: "GPT Custom".to_string(),
            efforts: vec!["high".to_string()],
            base_instructions: "custom base".to_string(),
        });

        let config = ProviderSettingsEdit {
            default_provider: Some("openai".to_string()),
            providers: vec![edit],
            roles: Vec::new(),
        }
        .to_config(&StudioConfig::default_config())
        .unwrap();

        let models = provider(&config, "openai").effective_models().unwrap();
        let custom = models.last().expect("custom model");
        assert_eq!(custom.slug, "gpt-custom");
        assert_eq!(custom.base_instructions, "custom base");
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

    #[test]
    fn zhipu_edit_keeps_kind_endpoint_and_catalogue() {
        let config = ProviderSettingsEdit {
            default_provider: Some("zhipu".to_string()),
            providers: vec![zhipu_edit()],
            roles: Vec::new(),
        }
        .to_config(&StudioConfig::default_config())
        .unwrap();
        let provider = provider(&config, "zhipu");

        assert_eq!(
            provider.protocol().unwrap(),
            pl_model::ProviderWireProtocol::ChatCompletions
        );
        assert_eq!(provider.base_url, "https://open.bigmodel.cn/api/paas/v4");
        assert!(
            provider
                .effective_models()
                .unwrap()
                .iter()
                .any(|model| model.slug == "glm-4.7-flashx")
        );
        assert_eq!(route(&config, StudioRole::Planner).model, "glm-5.2");
    }

    #[test]
    fn explicit_role_routes_are_preserved() {
        let config = ProviderSettingsEdit {
            default_provider: Some("openai".to_string()),
            providers: vec![openai_edit()],
            roles: vec![RoleEdit {
                key: "planner".to_string(),
                provider: "openai".to_string(),
                model: "gpt-5.4-mini".to_string(),
                effort: "medium".to_string(),
            }],
        }
        .to_config(&StudioConfig::default_config())
        .unwrap();

        let planner = route(&config, StudioRole::Planner);
        assert_eq!(planner.provider.as_str(), "openai");
        assert_eq!(planner.model, "gpt-5.4-mini");
        assert_eq!(planner.effort.as_ref().unwrap().as_str(), "medium");
    }

    #[test]
    fn provider_template_inference_uses_canonical_provider_config() {
        let edited = zhipu_edit().to_provider_config(None).unwrap();
        assert_eq!(
            provider_template_kind(&edited.config),
            Some(template("zhipu"))
        );
    }

    #[test]
    fn custom_responses_provider_preserves_transport_metadata_and_explicit_models() {
        let current_id = ProviderId::new("gateway").unwrap();
        let mut current_info = ProviderInfo {
            protocol: ProviderWireProtocol::Responses,
            connection_mode: ProviderConnectionMode::Http,
            name: "Gateway".to_string(),
            base_url: "https://gateway.example/v1".to_string(),
            default_model: "gateway-model".to_string(),
            bearer_token: Some("old-secret".to_string()),
            http_headers: Some(std::collections::HashMap::from([(
                "x-tenant".to_string(),
                "team-a".to_string(),
            )])),
            tool_wire_policy: pl_model::ToolWirePolicy::FunctionFallback,
            apply_patch_tool_type: None,
            service_capabilities: Default::default(),
        };
        let mut model = ModelInfo::fallback("gateway-model");
        model.context_window = Some(256_000);
        current_info.default_model.clear();
        let mut current = StudioConfig::default_config();
        current.models.providers = BTreeMap::from([(
            current_id.clone(),
            ProviderConfig::from_provider_info(current_info, vec![model]),
        )]);
        current.models.routes = StudioRole::all()
            .into_iter()
            .map(|role| {
                (
                    role.id(),
                    ModelRouteConfig {
                        provider: current_id.clone(),
                        model: "gateway-model".to_string(),
                        effort: Some(ReasoningEffort::new("high")),
                    },
                )
            })
            .collect();
        let edit = ProviderEdit {
            key: "gateway".to_string(),
            original_key: None,
            preset: None,
            protocol: ProviderWireProtocol::Responses,
            connection_mode: ProviderConnectionMode::WebSocket,
            name: "Gateway 2".to_string(),
            base_url: Some("https://gateway.example/v2".to_string()),
            bearer_token: Some("new-secret".to_string()),
            capabilities: ProviderCapabilitySelection::Explicit(
                ProviderServiceCapabilities::default(),
            ),
            default_model: "gateway-model".to_string(),
            custom_models: vec![ProviderModelEdit {
                slug: "gateway-model".to_string(),
                display_name: "Gateway Model".to_string(),
                efforts: vec!["high".to_string()],
                base_instructions: String::new(),
            }],
        };

        let config = ProviderSettingsEdit {
            default_provider: Some("gateway".to_string()),
            providers: vec![edit],
            roles: Vec::new(),
        }
        .to_config(&current)
        .unwrap();
        let provider = provider(&config, "gateway");

        assert_eq!(provider.preset_id(), None);
        assert_eq!(
            provider.protocol().unwrap(),
            ProviderWireProtocol::Responses
        );
        assert_eq!(
            provider.connection_mode(),
            ProviderConnectionMode::WebSocket
        );
        assert_eq!(
            provider.http_headers,
            Some(std::collections::HashMap::from([(
                "x-tenant".to_string(),
                "team-a".to_string(),
            )]))
        );
        assert_eq!(
            provider.effective_models().unwrap()[0].context_window,
            Some(256_000)
        );
    }

    #[test]
    fn duplicate_custom_model_slug_is_rejected() {
        let mut edit = openai_edit();
        edit.custom_models.push(ProviderModelEdit {
            slug: "gpt-5.5".to_string(),
            display_name: "Duplicate".to_string(),
            efforts: vec!["high".to_string()],
            base_instructions: String::new(),
        });

        let error = ProviderSettingsEdit {
            default_provider: Some("openai".to_string()),
            providers: vec![edit],
            roles: Vec::new(),
        }
        .to_config(&StudioConfig::default_config())
        .unwrap_err()
        .to_string();

        assert!(error.contains("additional model conflicts with bundled model"));
    }
}
