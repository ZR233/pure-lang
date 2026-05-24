use std::collections::BTreeMap;
use std::collections::BTreeSet;

use pl_model::{ModelInfo, WireApi};
use pl_protocol::{PureError, Result};

use crate::config::{
    CONFIG_SCHEMA_VERSION, ModelConfig, ProviderConfig, PureConfig, ReasoningEffort, RoleConfig,
    RoleConfigs,
};
use crate::first_run::ProviderTemplateKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderModelEdit {
    pub slug: String,
    pub display_name: String,
    pub reasoning_efforts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderEdit {
    pub key: String,
    pub kind: ProviderTemplateKind,
    pub name: String,
    pub base_url: Option<String>,
    pub env_key: Option<String>,
    pub bearer_token: Option<String>,
    pub default_model: String,
    pub wire_api: String,
    pub custom_models: Vec<ProviderModelEdit>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSettingsEdit {
    pub default_provider: Option<String>,
    pub providers: Vec<ProviderEdit>,
}

pub fn infer_provider_template_kind(
    provider_key: &str,
    provider: &ProviderConfig,
) -> ProviderTemplateKind {
    let key = provider_key.to_ascii_lowercase();
    let name = provider.name.to_ascii_lowercase();
    if key.contains("openai")
        || name.contains("openai")
        || provider.default_model.starts_with("gpt-")
        || matches!(provider.wire_api, WireApi::Responses)
    {
        ProviderTemplateKind::OpenAi
    } else {
        ProviderTemplateKind::DeepSeek
    }
}

impl ProviderModelEdit {
    fn to_model_config(&self) -> Result<ModelConfig> {
        let slug = non_empty_trimmed(&self.slug, "model slug")?;
        let mut model = ModelConfig::from_model_info(ModelInfo::fallback(&slug));
        model.display_name = non_empty_trimmed(&self.display_name, "model display_name")?;
        model.reasoning_efforts = normalized_efforts(&self.reasoning_efforts);
        Ok(model)
    }
}

impl ProviderEdit {
    fn provider_key(&self) -> Result<String> {
        validate_provider_key(&self.key)
    }

    fn to_provider_config(&self) -> Result<ProviderConfig> {
        let provider_key = self.provider_key()?;
        let mut info = self.kind.provider_info();
        info.name = non_empty_trimmed(&self.name, "provider name")?;
        info.base_url = trim_optional(self.base_url.as_deref());
        info.env_key = trim_optional(self.env_key.as_deref());
        info.default_model = non_empty_trimmed(&self.default_model, "provider default_model")?;
        info.bearer_token = trim_optional(self.bearer_token.as_deref());
        info.wire_api = parse_wire_api(&self.wire_api)?;

        let mut models = self.kind.default_models()?;
        models.extend(
            self.custom_models
                .iter()
                .map(ProviderModelEdit::to_model_config)
                .collect::<Result<Vec<_>>>()?,
        );
        validate_models(&provider_key, &info.default_model, &models)?;

        Ok(ProviderConfig::from_provider_info(info, models))
    }
}

impl ProviderSettingsEdit {
    pub fn to_config(&self, current: &PureConfig) -> Result<PureConfig> {
        if self.providers.is_empty() {
            return Err(PureError::ConfigError(
                "at least one provider is required".to_string(),
            ));
        }

        let mut provider_keys = BTreeSet::new();
        let mut providers = BTreeMap::new();
        for provider in &self.providers {
            let provider_key = provider.provider_key()?;
            if !provider_keys.insert(provider_key.clone()) {
                return Err(PureError::ConfigError(format!(
                    "duplicate provider key: {provider_key}"
                )));
            }
            providers.insert(provider_key, provider.to_provider_config()?);
        }

        let fallback_provider = self
            .default_provider
            .as_deref()
            .map(str::trim)
            .filter(|key| providers.contains_key(*key))
            .or_else(|| {
                providers
                    .contains_key(&current.roles.planner.provider)
                    .then_some(current.roles.planner.provider.as_str())
            })
            .or_else(|| providers.keys().next().map(String::as_str))
            .ok_or_else(|| PureError::ConfigError("at least one provider is required".to_string()))?
            .to_string();

        let roles = RoleConfigs {
            explorer: reconciled_role(&current.roles.explorer, &providers, &fallback_provider)?,
            planner: reconciled_role(&current.roles.planner, &providers, &fallback_provider)?,
            executor: reconciled_role(&current.roles.executor, &providers, &fallback_provider)?,
            reviewer: reconciled_role(&current.roles.reviewer, &providers, &fallback_provider)?,
        };
        let config = PureConfig {
            schema_version: CONFIG_SCHEMA_VERSION,
            roles,
            providers,
        };
        config.validate()?;
        Ok(config)
    }
}

fn reconciled_role(
    role: &RoleConfig,
    providers: &BTreeMap<String, ProviderConfig>,
    fallback_provider: &str,
) -> Result<RoleConfig> {
    if let Some(provider) = providers.get(&role.provider)
        && let Some(model) = provider
            .models
            .iter()
            .find(|model| model.slug == role.model)
    {
        if model
            .reasoning_efforts
            .iter()
            .any(|effort| effort == role.effort.as_str())
        {
            return Ok(role.clone());
        }
        if let Some(effort) = model.reasoning_efforts.first() {
            return Ok(RoleConfig {
                provider: role.provider.clone(),
                model: role.model.clone(),
                effort: ReasoningEffort::new(effort.clone()),
            });
        }
    }

    let provider = providers.get(fallback_provider).ok_or_else(|| {
        PureError::ConfigError(format!(
            "default provider references missing provider: {fallback_provider}"
        ))
    })?;
    let model = provider
        .models
        .iter()
        .find(|model| model.slug == provider.default_model)
        .ok_or_else(|| {
            PureError::ConfigError(format!(
                "default model is missing from provider: {}",
                provider.default_model
            ))
        })?;
    let effort = model.reasoning_efforts.first().cloned().ok_or_else(|| {
        PureError::ConfigError(format!(
            "default model {} must define reasoning_efforts",
            provider.default_model
        ))
    })?;

    Ok(RoleConfig {
        provider: fallback_provider.to_string(),
        model: provider.default_model.clone(),
        effort: ReasoningEffort::new(effort),
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

fn validate_models(provider_key: &str, default_model: &str, models: &[ModelConfig]) -> Result<()> {
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
    if model.reasoning_efforts.is_empty() {
        return Err(PureError::ConfigError(format!(
            "provider {provider_key} default model {default_model} must define reasoning_efforts"
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

fn parse_wire_api(value: &str) -> Result<WireApi> {
    match value.trim() {
        "chat" => Ok(WireApi::Chat),
        "responses" => Ok(WireApi::Responses),
        other => Err(PureError::ConfigError(format!(
            "unsupported wire_api: {other}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn openai_edit() -> ProviderEdit {
        ProviderEdit {
            key: "openai".to_string(),
            kind: ProviderTemplateKind::OpenAi,
            name: "OpenAI".to_string(),
            base_url: Some("https://api.openai.com/v1".to_string()),
            env_key: Some("API_KEY_OPENAI".to_string()),
            bearer_token: None,
            default_model: "gpt-5.5".to_string(),
            wire_api: "responses".to_string(),
            custom_models: Vec::new(),
        }
    }

    #[test]
    fn provider_edit_appends_custom_models_after_template_defaults() {
        let mut edit = openai_edit();
        edit.custom_models.push(ProviderModelEdit {
            slug: "gpt-custom".to_string(),
            display_name: "GPT Custom".to_string(),
            reasoning_efforts: vec!["high".to_string()],
        });
        let current = PureConfig::default_config();

        let config = ProviderSettingsEdit {
            default_provider: Some("openai".to_string()),
            providers: vec![edit],
        }
        .to_config(&current)
        .unwrap();

        let slugs = config.providers["openai"]
            .models
            .iter()
            .map(|model| model.slug.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            slugs,
            vec![
                "gpt-5.5",
                "gpt-5.4",
                "gpt-5.4-mini",
                "gpt-5.4-nano",
                "gpt-custom"
            ]
        );
    }

    #[test]
    fn provider_edit_repoints_roles_when_provider_is_removed() {
        let current = PureConfig::default_config();

        let config = ProviderSettingsEdit {
            default_provider: Some("openai".to_string()),
            providers: vec![openai_edit()],
        }
        .to_config(&current)
        .unwrap();

        assert_eq!(config.roles.planner.provider, "openai");
        assert_eq!(config.roles.planner.model, "gpt-5.5");
    }

    #[test]
    fn provider_edit_rejects_duplicate_custom_model_slug() {
        let mut edit = openai_edit();
        edit.custom_models.push(ProviderModelEdit {
            slug: "gpt-5.5".to_string(),
            display_name: "Duplicate".to_string(),
            reasoning_efforts: vec!["high".to_string()],
        });
        let current = PureConfig::default_config();

        let error = ProviderSettingsEdit {
            default_provider: Some("openai".to_string()),
            providers: vec![edit],
        }
        .to_config(&current)
        .unwrap_err()
        .to_string();

        assert!(error.contains("duplicate model slug"));
    }
}
