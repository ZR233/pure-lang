use std::collections::BTreeMap;
use std::collections::BTreeSet;

use pl_model::{
    ModelInfo, ProviderInfo, deepseek_default_model_slugs, default_models,
    openai_default_model_slugs, zhipu_default_model_slugs,
};
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
    Zhipu,
    ZhipuCodingPlan,
}

impl ProviderTemplateKind {
    pub fn all() -> [Self; 4] {
        [
            Self::DeepSeek,
            Self::OpenAi,
            Self::Zhipu,
            Self::ZhipuCodingPlan,
        ]
    }

    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "deepseek" => Some(Self::DeepSeek),
            "openai" => Some(Self::OpenAi),
            "zhipu" => Some(Self::Zhipu),
            "zhipu-coding-plan" | "zhipu_coding_plan" => Some(Self::ZhipuCodingPlan),
            _ => None,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::DeepSeek => "deepseek",
            Self::OpenAi => "openai",
            Self::Zhipu => "zhipu",
            Self::ZhipuCodingPlan => "zhipu-coding-plan",
        }
    }

    pub fn key_prefix(self) -> &'static str {
        match self {
            Self::DeepSeek => "deepseek",
            Self::OpenAi => "openai",
            Self::Zhipu => "zhipu",
            Self::ZhipuCodingPlan => "zhipu-coding-plan",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::DeepSeek => "DeepSeek",
            Self::OpenAi => "OpenAI",
            Self::Zhipu => "Zhipu",
            Self::ZhipuCodingPlan => "Zhipu Coding Plan",
        }
    }

    pub(crate) fn provider_info(self) -> ProviderInfo {
        match self {
            Self::DeepSeek => ProviderInfo::deepseek(None),
            Self::OpenAi => ProviderInfo::openai(None),
            Self::Zhipu => ProviderInfo::zhipu(None),
            Self::ZhipuCodingPlan => ProviderInfo::zhipu_coding_plan(None),
        }
    }

    pub fn default_models(self) -> Result<Vec<ModelConfig>> {
        let bundled_models = default_models();
        self.default_model_slugs()
            .iter()
            .map(|slug| {
                bundled_models
                    .iter()
                    .find(|model| model.slug == *slug)
                    .cloned()
                    .map(ModelConfig::from_model_info)
                    .ok_or_else(|| {
                        PureError::ConfigError(format!("default model template is missing: {slug}"))
                    })
            })
            .collect()
    }

    pub fn default_model_slugs(self) -> &'static [&'static str] {
        match self {
            Self::DeepSeek => deepseek_default_model_slugs(),
            Self::OpenAi => openai_default_model_slugs(),
            Self::Zhipu | Self::ZhipuCodingPlan => zhipu_default_model_slugs(),
        }
    }

    pub fn provider_config(self) -> Result<ProviderConfig> {
        Ok(ProviderConfig::from_provider_info(
            self.provider_info(),
            self.default_models()?,
        ))
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
            base_url: Some(info.base_url),
            bearer_token: String::new(),
            default_model: info.default_model,
            models: Vec::new(),
        }
    }

    pub fn all_models(&self) -> Result<Vec<ModelConfig>> {
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

        let mut info = self.kind.provider_info();
        info.name = non_empty_trimmed(&self.name, "provider name")?;
        info.base_url = trim_optional(self.base_url.as_deref()).ok_or_else(|| {
            PureError::ConfigError(format!("provider {} base_url must not be empty", self.key))
        })?;
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

        let mut config = PureConfig {
            schema_version: CONFIG_SCHEMA_VERSION,
            runtime: Default::default(),
            skills: Default::default(),
            mcp_servers: Default::default(),
            builtin_mcp_servers: Default::default(),
            roles: RoleConfigs {
                explorer: role.clone(),
                planner: role.clone(),
                executor: role.clone(),
                reviewer: role,
            },
            providers,
        };
        crate::config::normalize_builtin_mcp_server_states(&mut config);
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
        assert!(
            config.providers["deepseek"]
                .models
                .iter()
                .any(|model| model.slug == "deepseek-v4-pro")
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
    fn zhipu_draft_uses_glm_51_and_provider_endpoint() {
        let mut draft = FirstRunConfigDraft {
            default_provider: "zhipu".to_string(),
            providers: vec![FirstRunProviderDraft::from_template(
                "zhipu",
                ProviderTemplateKind::Zhipu,
            )],
        };
        draft.providers[0].bearer_token = "sk-zhipu".to_string();

        let config = draft.to_config().unwrap();

        assert_eq!(config.role_config(ModelRole::Planner).provider, "zhipu");
        assert_eq!(config.role_config(ModelRole::Planner).model, "glm-5.1");
        assert_eq!(
            config.providers["zhipu"].base_url,
            "https://open.bigmodel.cn/api/paas/v4"
        );
        assert!(
            config.providers["zhipu"]
                .models
                .iter()
                .any(|model| model.slug == "glm-4.7")
        );
    }

    #[test]
    fn zhipu_coding_plan_draft_reuses_zhipu_models_with_coding_endpoint() {
        let mut draft = FirstRunConfigDraft {
            default_provider: "zhipu-coding-plan".to_string(),
            providers: vec![FirstRunProviderDraft::from_template(
                "zhipu-coding-plan",
                ProviderTemplateKind::ZhipuCodingPlan,
            )],
        };
        draft.providers[0].bearer_token = "sk-coding-plan".to_string();

        let config = draft.to_config().unwrap();
        let zhipu_slugs = ProviderTemplateKind::Zhipu.default_model_slugs();
        let coding_plan_slugs = config.providers["zhipu-coding-plan"]
            .models
            .iter()
            .map(|model| model.slug.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            config.role_config(ModelRole::Planner).provider,
            "zhipu-coding-plan"
        );
        assert_eq!(config.role_config(ModelRole::Planner).model, "glm-5.1");
        assert_eq!(
            config.providers["zhipu-coding-plan"].base_url,
            "https://open.bigmodel.cn/api/coding/paas/v4"
        );
        assert_eq!(
            config.providers["zhipu-coding-plan"].provider_kind,
            pl_model::ProviderKind::Zhipu
        );
        assert_eq!(coding_plan_slugs, zhipu_slugs.to_vec());
    }

    #[test]
    fn repeated_provider_kind_gets_unique_suggested_key() {
        let mut draft = FirstRunConfigDraft::new_default();
        draft.add_provider(ProviderTemplateKind::DeepSeek);
        draft.add_provider(ProviderTemplateKind::OpenAi);
        draft.add_provider(ProviderTemplateKind::OpenAi);
        draft.add_provider(ProviderTemplateKind::Zhipu);
        draft.add_provider(ProviderTemplateKind::Zhipu);
        draft.add_provider(ProviderTemplateKind::ZhipuCodingPlan);
        draft.add_provider(ProviderTemplateKind::ZhipuCodingPlan);

        let keys = draft
            .providers
            .iter()
            .map(|provider| provider.key.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            keys,
            vec![
                "deepseek",
                "deepseek-2",
                "openai",
                "openai-2",
                "zhipu",
                "zhipu-2",
                "zhipu-coding-plan",
                "zhipu-coding-plan-2"
            ]
        );
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
