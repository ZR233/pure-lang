use std::collections::BTreeMap;
use std::collections::BTreeSet;

use crate::{PureError, Result};
use pl_model::{ModelInfo, ModelParameter, ProviderInfo};

use crate::config::{
    ModelRouteConfig, ProviderId, ReasoningEffort, STUDIO_CONFIG_SCHEMA_VERSION, StudioConfig,
    StudioRole,
};
use crate::{
    AgentModelConfig, ProviderConfig, ProviderModelCatalogConfig, ProviderPresetId,
    builtin_provider_catalog,
};

const DEFAULT_ROLE_EFFORT: &str = "high";

/// Studio 使用的动态 provider preset 标识。
///
/// 名称保留“Template”是为了描述 first-run 的用途；候选项完全来自 PL registry，
/// 不再由 Studio 枚举维护。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProviderTemplateKind(ProviderPresetId);

impl ProviderTemplateKind {
    pub fn all() -> Vec<Self> {
        builtin_provider_catalog()
            .presets
            .into_iter()
            .map(|preset| Self(preset.id))
            .collect()
    }

    pub fn from_key(key: &str) -> Option<Self> {
        let normalized = key.trim().replace('_', "-");
        builtin_provider_catalog()
            .presets
            .into_iter()
            .find(|preset| preset.id.as_str() == normalized)
            .map(|preset| Self(preset.id))
    }

    pub fn key(&self) -> &str {
        self.0.as_str()
    }

    pub fn key_prefix(&self) -> &str {
        self.key()
    }

    pub fn display_name(&self) -> String {
        self.preset().display_name
    }

    pub(crate) fn provider_info(&self) -> ProviderInfo {
        let preset = self.preset();
        preset
            .provider
            .to_provider_info(&preset.suggested_model)
            .expect("builtin provider preset must resolve")
    }

    pub fn default_models(&self) -> Result<Vec<ModelInfo>> {
        self.preset().provider.effective_models()
    }

    pub fn default_model_slugs(&self) -> Result<Vec<String>> {
        Ok(self
            .default_models()?
            .into_iter()
            .map(|model| model.slug)
            .collect())
    }

    pub fn provider_config(&self) -> ProviderConfig {
        self.preset().provider
    }

    fn preset(&self) -> crate::ProviderPreset {
        builtin_provider_catalog()
            .presets
            .into_iter()
            .find(|preset| preset.id == self.0)
            .expect("ProviderTemplateKind is created from the builtin registry")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FirstRunModelDraft {
    pub config: ModelInfo,
}

impl FirstRunModelDraft {
    pub fn from_model_info(config: ModelInfo) -> Self {
        Self { config }
    }

    pub fn fallback(slug: impl Into<String>) -> Self {
        let slug = slug.into();
        let mut model = ModelInfo::fallback(&slug);
        model.parameters = vec![fallback_effort_parameter()];
        Self { config: model }
    }
}

/// 占位模型的 effort 参数声明：单一候选值，无 wire（不真正发请求）。
fn fallback_effort_parameter() -> ModelParameter {
    ModelParameter {
        name: "effort".to_string(),
        label: None,
        candidates: vec![DEFAULT_ROLE_EFFORT.to_string()],
        wire: BTreeMap::new(),
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
            base_url: Some(info.base_url),
            bearer_token: String::new(),
            default_model: info.default_model,
            models: Vec::new(),
        }
    }

    pub fn all_models(&self) -> Result<Vec<ModelInfo>> {
        let mut models = self.kind.default_models()?;
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

        let mut provider = self.kind.provider_config();
        provider.name = non_empty_trimmed(&self.name, "provider name")?;
        provider.base_url = trim_optional(self.base_url.as_deref()).ok_or_else(|| {
            let key = &self.key;
            PureError::ConfigError(format!("provider {key} base_url must not be empty"))
        })?;
        let default_model = non_empty_trimmed(&self.default_model, "provider default_model")?;
        provider.bearer_token = Some(bearer_token.to_string());
        match &mut provider.catalog {
            ProviderModelCatalogConfig::Bundled {
                additional_models, ..
            } => {
                *additional_models = self
                    .models
                    .iter()
                    .map(|model| model.config.clone())
                    .collect();
            }
            ProviderModelCatalogConfig::Explicit { models } => {
                *models = self
                    .models
                    .iter()
                    .map(|model| model.config.clone())
                    .collect();
            }
        }

        let models = provider.effective_models()?;
        validate_models(&self.key, &default_model, &models)?;

        Ok(provider)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FirstRunConfigDraft {
    pub default_provider: String,
    pub providers: Vec<FirstRunProviderDraft>,
}

impl FirstRunConfigDraft {
    pub fn new_default() -> Self {
        let kind = ProviderTemplateKind::from_key("deepseek")
            .expect("PL builtin registry contains deepseek preset");
        let provider = FirstRunProviderDraft::from_template("deepseek", kind);
        Self {
            default_provider: provider.key.clone(),
            providers: vec![provider],
        }
    }

    pub fn suggest_provider_key(&self, kind: &ProviderTemplateKind) -> String {
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
        let key = self.suggest_provider_key(&kind);
        self.providers
            .push(FirstRunProviderDraft::from_template(key, kind));
        self.providers.last_mut().expect("provider was just pushed")
    }

    pub fn to_config(&self) -> Result<StudioConfig> {
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
            providers.insert(
                ProviderId::new(provider.key.clone())?,
                provider.to_provider_config()?,
            );
        }

        let default_provider_id = ProviderId::new(default_provider_key.clone())?;
        let default_provider = providers.get(&default_provider_id).ok_or_else(|| {
            PureError::ConfigError(format!(
                "default provider references missing provider: {default_provider_key}"
            ))
        })?;
        let default_model = self
            .providers
            .iter()
            .find(|provider| provider.key == default_provider_key)
            .map(|provider| provider.default_model.trim().to_string())
            .filter(|model| !model.is_empty())
            .ok_or_else(|| {
                PureError::ConfigError(format!(
                    "default provider has no default model: {default_provider_key}"
                ))
            })?;
        let default_effort = role_effort(default_provider, &default_model)?;
        let route = ModelRouteConfig {
            provider: default_provider_id,
            model: default_model,
            effort: Some(ReasoningEffort::new(default_effort)),
        };

        let config = StudioConfig {
            schema_version: STUDIO_CONFIG_SCHEMA_VERSION,
            models: AgentModelConfig {
                providers,
                routes: StudioRole::all()
                    .into_iter()
                    .map(|role| (role.id(), route.clone()))
                    .collect(),
            },
            web_search: Default::default(),
            runtime: Default::default(),
            instructions: Default::default(),
            skills: Default::default(),
            mcp: Default::default(),
            ui: Default::default(),
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

fn role_effort(provider: &ProviderConfig, model_slug: &str) -> Result<String> {
    let models = provider.effective_models()?;
    let model = models
        .iter()
        .find(|model| model.slug == model_slug)
        .ok_or_else(|| {
            PureError::ConfigError(format!(
                "default model is missing from provider: {model_slug}"
            ))
        })?;
    model.default_effort().ok_or_else(|| {
        PureError::ConfigError(format!(
            "default model {model_slug} must define effort parameter"
        ))
    })
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

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
    fn deepseek_draft_builds_valid_composed_config() {
        let mut draft = FirstRunConfigDraft::new_default();
        draft.providers[0].bearer_token = "sk-deepseek".to_string();

        let config = draft.to_config().unwrap();

        assert_eq!(
            route(&config, StudioRole::Planner).provider.as_str(),
            "deepseek"
        );
        assert_eq!(
            route(&config, StudioRole::Planner).model,
            "deepseek-v4-flash"
        );
        assert_eq!(
            provider(&config, "deepseek").bearer_token.as_deref(),
            Some("sk-deepseek")
        );
        assert!(
            provider(&config, "deepseek")
                .effective_models()
                .unwrap()
                .iter()
                .any(|model| model.slug == "deepseek-v4-pro")
        );
    }
}
