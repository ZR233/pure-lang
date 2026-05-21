use std::collections::BTreeMap;
use std::collections::BTreeSet;

use pl_model::{ModelInfo, ProviderInfo, default_models};
use pl_protocol::{PureError, Result};

use crate::config::{
    CONFIG_SCHEMA_VERSION, ModelConfig, ProviderConfig, PureConfig, ReasoningEffort, RoleConfig,
    RoleConfigs,
};

const DEFAULT_ROLE_EFFORT: &str = "high";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderTemplateKind {
    DeepSeek,
    OpenAi,
}

impl ProviderTemplateKind {
    pub fn key_prefix(self) -> &'static str {
        match self {
            Self::DeepSeek => "deepseek",
            Self::OpenAi => "openai",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::DeepSeek => "DeepSeek",
            Self::OpenAi => "OpenAI",
        }
    }

    fn provider_info(self) -> ProviderInfo {
        match self {
            Self::DeepSeek => ProviderInfo::deepseek(None),
            Self::OpenAi => ProviderInfo::openai(None),
        }
    }

    fn template_model(self) -> Result<ModelConfig> {
        let info = self.provider_info();
        default_models()
            .into_iter()
            .find(|model| model.slug == info.default_model)
            .map(ModelConfig::from_model_info)
            .ok_or_else(|| {
                PureError::ConfigError(format!(
                    "default model template is missing: {}",
                    info.default_model
                ))
            })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FirstRunModelDraft {
    pub config: ModelConfig,
}

impl FirstRunModelDraft {
    pub fn from_model_config(config: ModelConfig) -> Self {
        Self { config }
    }

    pub fn fallback(slug: impl Into<String>) -> Self {
        let slug = slug.into();
        let mut model = ModelConfig::from_model_info(ModelInfo::fallback(&slug));
        model.reasoning_efforts = vec![DEFAULT_ROLE_EFFORT.to_string()];
        Self { config: model }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FirstRunProviderDraft {
    pub key: String,
    pub kind: ProviderTemplateKind,
    pub name: String,
    pub base_url: Option<String>,
    pub bearer_token: String,
    pub default_model: String,
    pub models: Vec<FirstRunModelDraft>,
}

impl FirstRunProviderDraft {
    pub fn from_template(key: impl Into<String>, kind: ProviderTemplateKind) -> Self {
        let info = kind.provider_info();
        Self {
            key: key.into(),
            kind,
            name: info.name,
            base_url: info.base_url,
            bearer_token: String::new(),
            default_model: info.default_model,
            models: Vec::new(),
        }
    }

    pub fn template_model(&self) -> Result<ModelConfig> {
        self.kind.template_model()
    }

    pub fn all_models(&self) -> Result<Vec<ModelConfig>> {
        let mut models = vec![self.template_model()?];
        models.extend(self.models.iter().map(|model| model.config.clone()));
        Ok(models)
    }

    fn to_provider_config(&self) -> Result<ProviderConfig> {
        validate_provider_key(&self.key)?;
        let bearer_token = self.bearer_token.trim();
        if bearer_token.is_empty() {
            return Err(PureError::ConfigError(format!(
                "provider {} api key must not be empty",
                self.key
            )));
        }

        let mut info = self.kind.provider_info();
        info.name = non_empty_trimmed(&self.name, "provider name")?;
        info.base_url = trim_optional(self.base_url.as_deref());
        info.default_model = non_empty_trimmed(&self.default_model, "provider default_model")?;
        info.bearer_token = Some(bearer_token.to_string());

        let models = self.all_models()?;
        validate_models(&self.key, &info.default_model, &models)?;

        Ok(ProviderConfig::from_provider_info(info, models))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FirstRunConfigDraft {
    pub default_provider: String,
    pub providers: Vec<FirstRunProviderDraft>,
}

impl FirstRunConfigDraft {
    pub fn new_default() -> Self {
        let provider =
            FirstRunProviderDraft::from_template("deepseek", ProviderTemplateKind::DeepSeek);
        Self {
            default_provider: provider.key.clone(),
            providers: vec![provider],
        }
    }

    pub fn suggest_provider_key(&self, kind: ProviderTemplateKind) -> String {
        let prefix = kind.key_prefix();
        if !self.providers.iter().any(|provider| provider.key == prefix) {
            return prefix.to_string();
        }

        for index in 2.. {
            let candidate = format!("{prefix}-{index}");
            if !self
                .providers
                .iter()
                .any(|provider| provider.key == candidate)
            {
                return candidate;
            }
        }

        unreachable!("unbounded provider key suggestion should always return")
    }

    pub fn add_provider(&mut self, kind: ProviderTemplateKind) -> &mut FirstRunProviderDraft {
        let key = self.suggest_provider_key(kind);
        self.providers
            .push(FirstRunProviderDraft::from_template(key, kind));
        self.providers.last_mut().expect("provider was just pushed")
    }

    pub fn to_config(&self) -> Result<PureConfig> {
        if self.providers.is_empty() {
            return Err(PureError::ConfigError(
                "at least one provider is required".to_string(),
            ));
        }

        let default_provider_key = non_empty_trimmed(&self.default_provider, "default_provider")?;
        let mut provider_keys = BTreeSet::new();
        let mut providers = BTreeMap::new();

        for provider in &self.providers {
            if !provider_keys.insert(provider.key.clone()) {
                return Err(PureError::ConfigError(format!(
                    "duplicate provider key: {}",
                    provider.key
                )));
            }
            providers.insert(provider.key.clone(), provider.to_provider_config()?);
        }

        let default_provider = providers.get(&default_provider_key).ok_or_else(|| {
            PureError::ConfigError(format!(
                "default provider references missing provider: {default_provider_key}"
            ))
        })?;
        let default_model = default_provider.default_model.clone();
        let default_effort = role_effort(default_provider, &default_model)?;
        let role = RoleConfig {
            provider: default_provider_key,
            model: default_model,
            effort: ReasoningEffort::new(default_effort),
        };

        let config = PureConfig {
            schema_version: CONFIG_SCHEMA_VERSION,
            roles: RoleConfigs {
                explorer: role.clone(),
                planner: role.clone(),
                executor: role.clone(),
                reviewer: role,
            },
            providers,
        };
        config.validate()?;
        Ok(config)
    }
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

fn validate_provider_key(key: &str) -> Result<()> {
    let key = key.trim();
    if key.is_empty() {
        return Err(PureError::ConfigError(
            "provider key must not be empty".to_string(),
        ));
    }
    if !key
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    {
        return Err(PureError::ConfigError(format!(
            "provider key contains unsupported characters: {key}"
        )));
    }
    Ok(())
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

fn role_effort(provider: &ProviderConfig, model_slug: &str) -> Result<String> {
    let model = provider
        .models
        .iter()
        .find(|model| model.slug == model_slug)
        .ok_or_else(|| {
            PureError::ConfigError(format!(
                "default model is missing from provider: {model_slug}"
            ))
        })?;
    if model
        .reasoning_efforts
        .iter()
        .any(|effort| effort == DEFAULT_ROLE_EFFORT)
    {
        return Ok(DEFAULT_ROLE_EFFORT.to_string());
    }
    model.reasoning_efforts.first().cloned().ok_or_else(|| {
        PureError::ConfigError(format!(
            "default model {model_slug} must define reasoning_efforts"
        ))
    })
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::ModelRole;

    #[test]
    fn deepseek_draft_builds_valid_config() {
        let mut draft = FirstRunConfigDraft::new_default();
        draft.providers[0].bearer_token = "sk-deepseek".to_string();

        let config = draft.to_config().unwrap();

        assert_eq!(config.role_config(ModelRole::Planner).provider, "deepseek");
        assert_eq!(
            config.providers["deepseek"].bearer_token.as_deref(),
            Some("sk-deepseek")
        );
        assert_eq!(
            config.providers["deepseek"].default_model,
            "deepseek-v4-flash"
        );
    }

    #[test]
    fn openai_draft_uses_gpt_55_default_model() {
        let mut draft = FirstRunConfigDraft {
            default_provider: "openai".to_string(),
            providers: vec![FirstRunProviderDraft::from_template(
                "openai",
                ProviderTemplateKind::OpenAi,
            )],
        };
        draft.providers[0].bearer_token = "sk-openai".to_string();

        let config = draft.to_config().unwrap();

        assert_eq!(config.role_config(ModelRole::Planner).provider, "openai");
        assert_eq!(config.role_config(ModelRole::Planner).model, "gpt-5.5");
        assert_eq!(
            config.providers["openai"].bearer_token.as_deref(),
            Some("sk-openai")
        );
    }

    #[test]
    fn repeated_provider_kind_gets_unique_suggested_key() {
        let mut draft = FirstRunConfigDraft::new_default();
        draft.add_provider(ProviderTemplateKind::DeepSeek);
        draft.add_provider(ProviderTemplateKind::OpenAi);
        draft.add_provider(ProviderTemplateKind::OpenAi);

        let keys = draft
            .providers
            .iter()
            .map(|provider| provider.key.as_str())
            .collect::<Vec<_>>();

        assert_eq!(keys, vec!["deepseek", "deepseek-2", "openai", "openai-2"]);
    }

    #[test]
    fn roles_point_to_selected_default_provider() {
        let mut draft = FirstRunConfigDraft::new_default();
        draft.providers[0].bearer_token = "sk-deepseek".to_string();
        let openai = draft.add_provider(ProviderTemplateKind::OpenAi);
        openai.bearer_token = "sk-openai".to_string();
        draft.default_provider = "openai".to_string();

        let config = draft.to_config().unwrap();

        for role in ModelRole::all() {
            assert_eq!(config.role_config(role).provider, "openai");
            assert_eq!(config.role_config(role).model, "gpt-5.5");
        }
    }

    #[test]
    fn empty_api_key_is_rejected() {
        let draft = FirstRunConfigDraft::new_default();

        let error = draft.to_config().unwrap_err().to_string();

        assert!(error.contains("api key must not be empty"));
    }

    #[test]
    fn duplicate_provider_key_is_rejected() {
        let mut draft = FirstRunConfigDraft::new_default();
        draft.providers[0].bearer_token = "sk-1".to_string();
        let provider = draft.add_provider(ProviderTemplateKind::OpenAi);
        provider.key = "deepseek".to_string();
        provider.bearer_token = "sk-2".to_string();

        let error = draft.to_config().unwrap_err().to_string();

        assert!(error.contains("duplicate provider key"));
    }

    #[test]
    fn duplicate_model_slug_is_rejected() {
        let mut draft = FirstRunConfigDraft::new_default();
        draft.providers[0].bearer_token = "sk-deepseek".to_string();
        draft.providers[0]
            .models
            .push(FirstRunModelDraft::fallback("deepseek-v4-flash"));

        let error = draft.to_config().unwrap_err().to_string();

        assert!(error.contains("duplicate model slug"));
    }

    #[test]
    fn default_custom_model_requires_reasoning_efforts() {
        let mut draft = FirstRunConfigDraft::new_default();
        draft.providers[0].bearer_token = "sk-deepseek".to_string();
        let mut model = FirstRunModelDraft::fallback("custom-model");
        model.config.reasoning_efforts.clear();
        draft.providers[0].default_model = "custom-model".to_string();
        draft.providers[0].models.push(model);

        let error = draft.to_config().unwrap_err().to_string();

        assert!(error.contains("must define reasoning_efforts"));
    }
}
