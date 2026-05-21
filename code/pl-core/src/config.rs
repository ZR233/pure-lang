use std::collections::BTreeMap;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use pl_model::{
    AuthCommand, InputModality, ModelCapabilities, ModelInfo, ProviderInfo, TruncationMode,
    TruncationPolicy, WireApi, default_models,
};
use pl_protocol::{PureError, Result};
use serde::Deserialize;
use serde::Serialize;

pub const CONFIG_DIR_NAME: &str = ".pure";
pub const CONFIG_FILE_NAME: &str = "config.toml";
pub const CONFIG_SCHEMA_VERSION: u32 = 1;

const DEFAULT_PROVIDER_KEY: &str = "deepseek";
const DEFAULT_MODEL: &str = "deepseek-v4-flash";
const DEFAULT_EFFORT: &str = "high";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PureConfig {
    pub schema_version: u32,
    pub roles: RoleConfigs,
    pub providers: BTreeMap<String, ProviderConfig>,
}

impl PureConfig {
    pub fn default_config() -> Self {
        let role = RoleConfig {
            provider: DEFAULT_PROVIDER_KEY.to_string(),
            model: DEFAULT_MODEL.to_string(),
            effort: ReasoningEffort::new(DEFAULT_EFFORT),
        };
        let mut providers = BTreeMap::new();
        providers.insert(
            DEFAULT_PROVIDER_KEY.to_string(),
            ProviderConfig::default_deepseek(),
        );

        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            roles: RoleConfigs {
                explorer: role.clone(),
                planner: role.clone(),
                executor: role.clone(),
                reviewer: role,
            },
            providers,
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema_version != CONFIG_SCHEMA_VERSION {
            return Err(PureError::ConfigError(format!(
                "unsupported config schema version: {}",
                self.schema_version
            )));
        }

        for (provider_key, provider) in &self.providers {
            provider.validate(provider_key)?;
        }

        for role in ModelRole::all() {
            self.resolve_role(role)?;
        }

        Ok(())
    }

    pub fn role_config(&self, role: ModelRole) -> &RoleConfig {
        self.roles.get(role)
    }

    pub fn resolve_role(&self, role: ModelRole) -> Result<ResolvedRoleConfig> {
        let role_config = self.role_config(role);
        let provider = self.providers.get(&role_config.provider).ok_or_else(|| {
            PureError::ConfigError(format!(
                "role {} references missing provider: {}",
                role.key(),
                role_config.provider
            ))
        })?;
        let model = provider
            .models
            .iter()
            .find(|model| model.slug == role_config.model)
            .ok_or_else(|| {
                PureError::ConfigError(format!(
                    "role {} references missing model: {}.{}",
                    role.key(),
                    role_config.provider,
                    role_config.model
                ))
            })?;

        if !model
            .reasoning_efforts
            .iter()
            .any(|effort| effort == role_config.effort.as_str())
        {
            return Err(PureError::ConfigError(format!(
                "role {} uses unsupported effort '{}' for model {}.{}",
                role.key(),
                role_config.effort.as_str(),
                role_config.provider,
                role_config.model
            )));
        }

        Ok(ResolvedRoleConfig {
            role,
            role_config: role_config.clone(),
            provider_key: role_config.provider.clone(),
            provider_info: provider.to_provider_info(&role_config.model),
            models: provider
                .models
                .iter()
                .cloned()
                .map(ModelConfig::into_model_info)
                .collect(),
        })
    }

    pub fn to_toml_pretty(&self) -> Result<String> {
        toml::to_string_pretty(self)
            .map_err(|error| PureError::ConfigError(format!("failed to serialize config: {error}")))
    }

    pub fn from_toml(content: &str) -> Result<Self> {
        let config: Self = toml::from_str(content)
            .map_err(|error| PureError::ConfigError(format!("failed to parse config: {error}")))?;
        config.validate()?;
        Ok(config)
    }
}

impl Default for PureConfig {
    fn default() -> Self {
        Self::default_config()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RoleConfigs {
    pub explorer: RoleConfig,
    pub planner: RoleConfig,
    pub executor: RoleConfig,
    pub reviewer: RoleConfig,
}

impl RoleConfigs {
    pub fn get(&self, role: ModelRole) -> &RoleConfig {
        match role {
            ModelRole::Explorer => &self.explorer,
            ModelRole::Planner => &self.planner,
            ModelRole::Executor => &self.executor,
            ModelRole::Reviewer => &self.reviewer,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RoleConfig {
    pub provider: String,
    pub model: String,
    pub effort: ReasoningEffort,
}

#[derive(Debug, Clone)]
pub struct ResolvedRoleConfig {
    pub role: ModelRole,
    pub role_config: RoleConfig,
    pub provider_key: String,
    pub provider_info: ProviderInfo,
    pub models: Vec<ModelInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelRole {
    Explorer,
    Planner,
    Executor,
    Reviewer,
}

impl ModelRole {
    pub fn all() -> [Self; 4] {
        [
            Self::Explorer,
            Self::Planner,
            Self::Executor,
            Self::Reviewer,
        ]
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Explorer => "explorer",
            Self::Planner => "planner",
            Self::Executor => "executor",
            Self::Reviewer => "reviewer",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Explorer => "探索者",
            Self::Planner => "计划者",
            Self::Executor => "执行者",
            Self::Reviewer => "审查者",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct ReasoningEffort(String);

impl ReasoningEffort {
    pub fn new(effort: impl Into<String>) -> Self {
        Self(effort.into())
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub fn is_none(&self) -> bool {
        self.as_str() == "none"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderConfig {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env_key_instructions: Option<String>,
    pub default_model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bearer_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_command: Option<AuthCommand>,
    #[serde(default)]
    pub wire_api: WireApi,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_headers: Option<HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env_http_headers: Option<HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_max_retries: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_max_retries: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_idle_timeout_ms: Option<u64>,
    #[serde(default)]
    pub models: Vec<ModelConfig>,
}

impl ProviderConfig {
    pub fn default_deepseek() -> Self {
        let info = ProviderInfo::deepseek(None);
        let models = default_models()
            .into_iter()
            .filter(|model| model.slug == info.default_model)
            .map(ModelConfig::from_model_info)
            .collect();
        Self::from_provider_info(info, models)
    }

    pub fn from_provider_info(info: ProviderInfo, models: Vec<ModelConfig>) -> Self {
        Self {
            name: info.name,
            base_url: info.base_url,
            env_key: info.env_key,
            env_key_instructions: info.env_key_instructions,
            default_model: info.default_model,
            bearer_token: info.bearer_token,
            auth_command: info.auth_command,
            wire_api: info.wire_api,
            http_headers: info.http_headers,
            env_http_headers: info.env_http_headers,
            request_max_retries: info.request_max_retries,
            stream_max_retries: info.stream_max_retries,
            stream_idle_timeout_ms: info.stream_idle_timeout_ms,
            models,
        }
    }

    pub fn to_provider_info(&self, default_model: &str) -> ProviderInfo {
        ProviderInfo {
            name: self.name.clone(),
            base_url: self.base_url.clone(),
            env_key: self.env_key.clone(),
            env_key_instructions: self.env_key_instructions.clone(),
            default_model: default_model.to_string(),
            bearer_token: self.bearer_token.clone(),
            auth_command: self.auth_command.clone(),
            wire_api: self.wire_api,
            http_headers: self.http_headers.clone(),
            env_http_headers: self.env_http_headers.clone(),
            request_max_retries: self.request_max_retries,
            stream_max_retries: self.stream_max_retries,
            stream_idle_timeout_ms: self.stream_idle_timeout_ms,
        }
    }

    fn validate(&self, provider_key: &str) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(PureError::ConfigError(format!(
                "provider {provider_key} has empty name"
            )));
        }
        if self.default_model.trim().is_empty() {
            return Err(PureError::ConfigError(format!(
                "provider {provider_key} has empty default_model"
            )));
        }
        if self.models.is_empty() {
            return Err(PureError::ConfigError(format!(
                "provider {provider_key} must define at least one model"
            )));
        }
        if !self
            .models
            .iter()
            .any(|model| model.slug == self.default_model)
        {
            return Err(PureError::ConfigError(format!(
                "provider {provider_key} default_model is not in models: {}",
                self.default_model
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelConfig {
    pub slug: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_context_window: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_compact_token_limit: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_temperature: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
    #[serde(default)]
    pub reasoning_efforts: Vec<String>,
    #[serde(default)]
    pub capabilities: Vec<ModelCapabilityConfig>,
    #[serde(default)]
    pub input_modalities: Vec<InputModality>,
    pub truncation_policy: TruncationPolicyConfig,
    #[serde(default)]
    pub base_instructions: String,
}

impl ModelConfig {
    pub fn from_model_info(model: ModelInfo) -> Self {
        Self {
            slug: model.slug,
            display_name: model.display_name,
            description: model.description,
            context_window: model.context_window,
            max_context_window: model.max_context_window,
            auto_compact_token_limit: model.auto_compact_token_limit,
            default_temperature: model.default_temperature,
            max_output_tokens: model.max_output_tokens,
            reasoning_efforts: model.reasoning_efforts,
            capabilities: ModelCapabilityConfig::from_capabilities(model.capabilities),
            input_modalities: model.input_modalities,
            truncation_policy: TruncationPolicyConfig::from_policy(model.truncation_policy),
            base_instructions: model.base_instructions,
        }
    }

    pub fn into_model_info(self) -> ModelInfo {
        ModelInfo {
            slug: self.slug,
            display_name: self.display_name,
            description: self.description,
            context_window: self.context_window,
            max_context_window: self.max_context_window,
            auto_compact_token_limit: self.auto_compact_token_limit,
            default_temperature: self.default_temperature,
            max_output_tokens: self.max_output_tokens,
            reasoning_efforts: self.reasoning_efforts,
            capabilities: ModelCapabilityConfig::to_capabilities(&self.capabilities),
            input_modalities: self.input_modalities,
            truncation_policy: self.truncation_policy.into_policy(),
            base_instructions: self.base_instructions,
            used_fallback: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelCapabilityConfig {
    Streaming,
    FunctionCalling,
    Vision,
    ParallelToolCalls,
    Reasoning,
    WebSearch,
}

impl ModelCapabilityConfig {
    fn from_capabilities(capabilities: ModelCapabilities) -> Vec<Self> {
        [
            (ModelCapabilities::STREAMING, Self::Streaming),
            (ModelCapabilities::FUNCTION_CALLING, Self::FunctionCalling),
            (ModelCapabilities::VISION, Self::Vision),
            (
                ModelCapabilities::PARALLEL_TOOL_CALLS,
                Self::ParallelToolCalls,
            ),
            (ModelCapabilities::REASONING, Self::Reasoning),
            (ModelCapabilities::WEB_SEARCH, Self::WebSearch),
        ]
        .into_iter()
        .filter_map(|(flag, config)| capabilities.contains(flag).then_some(config))
        .collect()
    }

    fn to_capabilities(configs: &[Self]) -> ModelCapabilities {
        configs
            .iter()
            .fold(ModelCapabilities::empty(), |capabilities, config| {
                capabilities
                    | match config {
                        Self::Streaming => ModelCapabilities::STREAMING,
                        Self::FunctionCalling => ModelCapabilities::FUNCTION_CALLING,
                        Self::Vision => ModelCapabilities::VISION,
                        Self::ParallelToolCalls => ModelCapabilities::PARALLEL_TOOL_CALLS,
                        Self::Reasoning => ModelCapabilities::REASONING,
                        Self::WebSearch => ModelCapabilities::WEB_SEARCH,
                    }
            })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TruncationPolicyConfig {
    pub mode: TruncationMode,
    pub limit: u64,
}

impl TruncationPolicyConfig {
    fn from_policy(policy: TruncationPolicy) -> Self {
        Self {
            mode: policy.mode,
            limit: policy.limit,
        }
    }

    fn into_policy(self) -> TruncationPolicy {
        TruncationPolicy {
            mode: self.mode,
            limit: self.limit,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigPaths {
    config_dir: PathBuf,
    config_file: PathBuf,
}

impl ConfigPaths {
    pub fn for_current_user() -> Result<Self> {
        let home = user_home_dir()?;
        Ok(Self::from_home(home))
    }

    pub fn from_home(home: impl Into<PathBuf>) -> Self {
        let config_dir = home.into().join(CONFIG_DIR_NAME);
        let config_file = config_dir.join(CONFIG_FILE_NAME);
        Self {
            config_dir,
            config_file,
        }
    }

    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    pub fn config_file(&self) -> &Path {
        &self.config_file
    }
}

#[derive(Debug, Clone)]
pub struct ConfigStore {
    paths: ConfigPaths,
}

impl ConfigStore {
    pub fn default_app() -> Result<Self> {
        Ok(Self::new(ConfigPaths::for_current_user()?))
    }

    pub fn new(paths: ConfigPaths) -> Self {
        Self { paths }
    }

    pub fn paths(&self) -> &ConfigPaths {
        &self.paths
    }

    pub fn load_or_default(&self) -> Result<PureConfig> {
        if self.paths.config_file().exists() {
            self.load()
        } else {
            Ok(PureConfig::default_config())
        }
    }

    pub fn load(&self) -> Result<PureConfig> {
        let content = fs::read_to_string(self.paths.config_file())?;
        PureConfig::from_toml(&content)
    }

    pub fn save(&self, config: &PureConfig) -> Result<()> {
        config.validate()?;
        fs::create_dir_all(self.paths.config_dir())?;
        fs::write(self.paths.config_file(), config.to_toml_pretty()?)?;
        Ok(())
    }

    pub fn init_default(&self) -> Result<PureConfig> {
        if self.paths.config_file().exists() {
            return Err(PureError::ConfigError(format!(
                "config already exists: {}",
                self.paths.config_file().display()
            )));
        }

        let config = PureConfig::default_config();
        self.save(&config)?;
        Ok(config)
    }
}

fn user_home_dir() -> Result<PathBuf> {
    #[cfg(windows)]
    const HOME_VARS: &[&str] = &["USERPROFILE", "HOME"];
    #[cfg(not(windows))]
    const HOME_VARS: &[&str] = &["HOME", "USERPROFILE"];

    HOME_VARS
        .iter()
        .filter_map(env::var_os)
        .map(PathBuf::from)
        .find(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| PureError::ConfigError("could not resolve user home directory".to_string()))
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn temp_home(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        env::temp_dir().join(format!("pure-lang-{name}-{}-{stamp}", std::process::id()))
    }

    #[test]
    fn default_path_uses_pure_directory_under_home() {
        let paths = ConfigPaths::from_home("C:/Users/example");

        assert!(paths.config_file().ends_with(".pure/config.toml"));
    }

    #[test]
    fn missing_config_loads_default_four_roles() {
        let store = ConfigStore::new(ConfigPaths::from_home(temp_home("missing")));
        let config = store.load_or_default().unwrap();

        assert_eq!(config.role_config(ModelRole::Planner).provider, "deepseek");
        assert_eq!(
            config.role_config(ModelRole::Explorer).effort.as_str(),
            "high"
        );
        assert_eq!(config.providers["deepseek"].models.len(), 1);
    }

    #[test]
    fn toml_round_trip_preserves_roles_models_and_token() {
        let mut config = PureConfig::default_config();
        config.providers.get_mut("deepseek").unwrap().bearer_token =
            Some("secret-token".to_string());

        let toml = config.to_toml_pretty().unwrap();
        let parsed = PureConfig::from_toml(&toml).unwrap();

        assert_eq!(
            parsed.providers["deepseek"].bearer_token.as_deref(),
            Some("secret-token")
        );
        assert_eq!(parsed.role_config(ModelRole::Reviewer).model, DEFAULT_MODEL);
        assert_eq!(
            parsed.providers["deepseek"].models[0].capabilities,
            config.providers["deepseek"].models[0].capabilities
        );
    }

    #[test]
    fn role_rejects_missing_model() {
        let mut config = PureConfig::default_config();
        config.roles.planner.model = "missing-model".to_string();

        let error = config.validate().unwrap_err().to_string();

        assert!(error.contains("missing model"));
    }

    #[test]
    fn role_rejects_unsupported_effort() {
        let mut config = PureConfig::default_config();
        config.roles.planner.effort = ReasoningEffort::new("xhigh");

        let error = config.validate().unwrap_err().to_string();

        assert!(error.contains("unsupported effort"));
    }

    #[test]
    fn init_default_writes_config_file() {
        let home = temp_home("init");
        let store = ConfigStore::new(ConfigPaths::from_home(&home));

        store.init_default().unwrap();

        assert!(home.join(".pure").join("config.toml").exists());
        fs::remove_dir_all(home).unwrap();
    }
}
