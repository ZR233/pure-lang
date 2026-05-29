use std::collections::BTreeMap;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use pl_model::{
    ApplyPatchToolType, AuthCommand, InputModality, ModelCapabilities, ModelInfo, ProviderInfo,
    TruncationMode, TruncationPolicy, WireApi, deepseek_default_model_slugs, default_models,
};
use pl_protocol::{PureError, Result};
use serde::Deserialize;
use serde::Serialize;

pub const CONFIG_DIR_NAME: &str = ".pure";
pub const CONFIG_FILE_NAME: &str = "config.toml";
pub const CONFIG_SCHEMA_VERSION: u32 = 2;
pub const LEGACY_CONFIG_SCHEMA_VERSION: u32 = 1;

const DEFAULT_PROVIDER_KEY: &str = "deepseek";
const DEFAULT_MODEL: &str = "deepseek-v4-flash";
const DEFAULT_EFFORT: &str = "high";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PureConfig {
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "RuntimeConfig::is_empty")]
    pub runtime: RuntimeConfig,
    pub roles: RoleConfigs,
    pub providers: BTreeMap<String, ProviderConfig>,
}

#[derive(Debug, Clone, Deserialize)]
struct PureConfigToml {
    pub schema_version: u32,
    #[serde(default)]
    pub runtime: RuntimeConfig,
    #[serde(default)]
    pub roles: Option<RoleConfigsToml>,
    pub providers: BTreeMap<String, ProviderConfig>,
}

#[derive(Debug, Clone, Deserialize)]
struct RoleConfigsToml {
    #[serde(default)]
    pub explorer: Option<RoleConfig>,
    #[serde(default)]
    pub planner: Option<RoleConfig>,
    #[serde(default)]
    pub executor: Option<RoleConfig>,
    #[serde(default)]
    pub reviewer: Option<RoleConfig>,
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
            runtime: RuntimeConfig::default(),
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
        let raw: PureConfigToml = toml::from_str(content)
            .map_err(|error| PureError::ConfigError(format!("failed to parse config: {error}")))?;
        if raw.schema_version != CONFIG_SCHEMA_VERSION {
            return Err(PureError::ConfigError(format!(
                "unsupported config schema version: {}",
                raw.schema_version
            )));
        }
        let roles = match raw.roles {
            Some(roles) => roles.into_role_configs(&raw.providers)?,
            None => RoleConfigs::from_default_role(default_role_config(&raw.providers)?),
        };
        let config = Self {
            schema_version: raw.schema_version,
            runtime: raw.runtime,
            roles,
            providers: raw.providers,
        };
        config.validate()?;
        Ok(config)
    }
}

impl Default for PureConfig {
    fn default() -> Self {
        Self::default_config()
    }
}

fn default_role_config(providers: &BTreeMap<String, ProviderConfig>) -> Result<RoleConfig> {
    let (provider_key, provider) = providers
        .iter()
        .next()
        .ok_or_else(|| PureError::ConfigError("at least one provider is required".to_string()))?;
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
        provider: provider_key.clone(),
        model: provider.default_model.clone(),
        effort: ReasoningEffort::new(effort),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RoleConfigs {
    pub explorer: RoleConfig,
    pub planner: RoleConfig,
    pub executor: RoleConfig,
    pub reviewer: RoleConfig,
}

impl RoleConfigs {
    pub fn from_default_role(role: RoleConfig) -> Self {
        Self {
            explorer: role.clone(),
            planner: role.clone(),
            executor: role.clone(),
            reviewer: role,
        }
    }

    pub fn get(&self, role: ModelRole) -> &RoleConfig {
        match role {
            ModelRole::Explorer => &self.explorer,
            ModelRole::Planner => &self.planner,
            ModelRole::Executor => &self.executor,
            ModelRole::Reviewer => &self.reviewer,
        }
    }
}

impl RoleConfigsToml {
    fn into_role_configs(
        self,
        providers: &BTreeMap<String, ProviderConfig>,
    ) -> Result<RoleConfigs> {
        let fallback_role = if self.explorer.is_none()
            || self.planner.is_none()
            || self.executor.is_none()
            || self.reviewer.is_none()
        {
            Some(default_role_config(providers)?)
        } else {
            None
        };
        Ok(RoleConfigs {
            explorer: role_or_default(self.explorer, &fallback_role),
            planner: role_or_default(self.planner, &fallback_role),
            executor: role_or_default(self.executor, &fallback_role),
            reviewer: role_or_default(self.reviewer, &fallback_role),
        })
    }
}

fn role_or_default(role: Option<RoleConfig>, fallback_role: &Option<RoleConfig>) -> RoleConfig {
    role.unwrap_or_else(|| {
        fallback_role
            .as_ref()
            .expect("fallback role exists when a role is missing")
            .clone()
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RoleConfig {
    pub provider: String,
    pub model: String,
    pub effort: ReasoningEffort,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeConfig {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub active_skills: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub active_mcp_servers: Vec<String>,
}

impl RuntimeConfig {
    pub fn is_empty(&self) -> bool {
        self.active_skills.is_empty() && self.active_mcp_servers.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedRoleConfig {
    pub role: ModelRole,
    pub role_config: RoleConfig,
    pub provider_key: String,
    pub provider_info: ProviderInfo,
    pub models: Vec<ModelInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
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

    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "explorer" => Some(Self::Explorer),
            "planner" => Some(Self::Planner),
            "executor" => Some(Self::Executor),
            "reviewer" => Some(Self::Reviewer),
            _ => None,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_custom_tools: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_freeform_tools: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apply_patch_tool_type: Option<ApplyPatchToolType>,
    #[serde(default)]
    pub models: Vec<ModelConfig>,
}

impl ProviderConfig {
    pub fn default_deepseek() -> Self {
        let info = ProviderInfo::deepseek(None);
        let slugs = deepseek_default_model_slugs();
        let models = default_models()
            .into_iter()
            .filter(|model| slugs.contains(&model.slug.as_str()))
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
            supports_custom_tools: info.supports_custom_tools,
            supports_freeform_tools: info.supports_freeform_tools,
            apply_patch_tool_type: info.apply_patch_tool_type,
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
            supports_custom_tools: self.supports_custom_tools,
            supports_freeform_tools: self.supports_freeform_tools,
            apply_patch_tool_type: self.apply_patch_tool_type,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_price_per_mtok: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_price_per_mtok: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_price_per_mtok: Option<f64>,
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
            currency: model.currency,
            input_price_per_mtok: model.input_price_per_mtok,
            output_price_per_mtok: model.output_price_per_mtok,
            cache_read_price_per_mtok: model.cache_read_price_per_mtok,
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
            currency: self.currency,
            input_price_per_mtok: self.input_price_per_mtok,
            output_price_per_mtok: self.output_price_per_mtok,
            cache_read_price_per_mtok: self.cache_read_price_per_mtok,
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
    CustomTools,
    FreeformTools,
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
            (ModelCapabilities::CUSTOM_TOOLS, Self::CustomTools),
            (ModelCapabilities::FREEFORM_TOOLS, Self::FreeformTools),
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
                        Self::CustomTools => ModelCapabilities::CUSTOM_TOOLS,
                        Self::FreeformTools => ModelCapabilities::FREEFORM_TOOLS,
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

    pub fn config_exists(&self) -> bool {
        self.paths.config_file().exists()
    }

    pub fn load_or_default(&self) -> Result<PureConfig> {
        if self.config_exists() {
            let content = fs::read_to_string(self.paths.config_file())?;
            match PureConfig::from_toml(&content) {
                Ok(config) => Ok(config),
                Err(error) => {
                    if parse_schema_version(&content)? == Some(LEGACY_CONFIG_SCHEMA_VERSION) {
                        let migrated = migrate_legacy_config(&content)?;
                        self.backup_legacy_config()?;
                        self.save(&migrated)?;
                        return Ok(migrated);
                    }
                    Err(error)
                }
            }
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

    fn backup_legacy_config(&self) -> Result<PathBuf> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let backup = self
            .paths
            .config_dir()
            .join(format!("config.v1.backup.{now}.toml"));
        fs::copy(self.paths.config_file(), &backup)?;
        Ok(backup)
    }
}

fn parse_schema_version(content: &str) -> Result<Option<u32>> {
    let value: toml::Value = toml::from_str(content)
        .map_err(|error| PureError::ConfigError(format!("failed to parse config: {error}")))?;
    Ok(value
        .as_table()
        .and_then(|table| table.get("schema_version"))
        .and_then(toml::Value::as_integer)
        .and_then(|value| u32::try_from(value).ok()))
}

fn migrate_legacy_config(content: &str) -> Result<PureConfig> {
    let value: toml::Value = toml::from_str(content).map_err(|error| {
        PureError::ConfigError(format!("failed to parse legacy config: {error}"))
    })?;
    let table = value
        .as_table()
        .ok_or_else(|| PureError::ConfigError("legacy config root must be table".to_string()))?;

    let mut migrated = PureConfig::default_config();

    if let Some(runtime_value) = table.get("runtime")
        && let Ok(runtime) = runtime_value.clone().try_into::<RuntimeConfig>()
    {
        migrated.runtime = runtime;
    }

    if let Some(providers_value) = table.get("providers")
        && let Ok(providers) = providers_value
            .clone()
            .try_into::<BTreeMap<String, ProviderConfig>>()
        && !providers.is_empty()
    {
        migrated.providers = providers;
    }

    if let Some(roles_value) = table.get("roles")
        && let Ok(roles_toml) = roles_value.clone().try_into::<RoleConfigsToml>()
    {
        migrated.roles = roles_toml.into_role_configs(&migrated.providers)?;
    } else {
        let fallback = default_role_config(&migrated.providers)?;
        migrated.roles = RoleConfigs::from_default_role(fallback);
    }

    migrated.schema_version = CONFIG_SCHEMA_VERSION;
    migrated.validate()?;
    Ok(migrated)
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

    fn without_section(content: &str, section: &str) -> String {
        let mut filtered = Vec::new();
        let mut skipping = false;
        for line in content.lines() {
            if line.trim() == section {
                skipping = true;
                continue;
            }
            if skipping && line.starts_with('[') {
                skipping = false;
            }
            if !skipping {
                filtered.push(line);
            }
        }
        filtered.join("\n")
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
        assert_eq!(config.providers["deepseek"].models.len(), 2);
        assert!(
            config.providers["deepseek"]
                .models
                .iter()
                .any(|model| model.slug == "deepseek-v4-pro")
        );
    }

    #[test]
    fn toml_round_trip_preserves_roles_models_and_token() {
        let mut config = PureConfig::default_config();
        config.providers.get_mut("deepseek").unwrap().bearer_token =
            Some("secret-token".to_string());
        config.runtime.active_skills = vec!["rust".to_string(), "git".to_string()];
        config.runtime.active_mcp_servers = vec!["github".to_string()];
        let model = &mut config.providers.get_mut("deepseek").unwrap().models[0];
        model.currency = Some("CNY".to_string());
        model.input_price_per_mtok = Some(1.0);
        model.output_price_per_mtok = Some(2.0);
        model.cache_read_price_per_mtok = Some(0.02);

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
        assert_eq!(
            parsed.runtime.active_skills,
            vec!["rust".to_string(), "git".to_string()]
        );
        assert_eq!(
            parsed.runtime.active_mcp_servers,
            vec!["github".to_string()]
        );
        assert_eq!(
            parsed.providers["deepseek"].models[0].currency.as_deref(),
            Some("CNY")
        );
        assert_eq!(
            parsed.providers["deepseek"].models[0].input_price_per_mtok,
            Some(1.0)
        );
    }

    #[test]
    fn missing_runtime_defaults_to_empty_lists() {
        let toml = PureConfig::default_config().to_toml_pretty().unwrap();
        let parsed = PureConfig::from_toml(&toml).unwrap();

        assert!(parsed.runtime.active_skills.is_empty());
        assert!(parsed.runtime.active_mcp_servers.is_empty());
    }

    #[test]
    fn missing_single_role_uses_first_provider_default_model() {
        let mut config = PureConfig::default_config();
        config.roles.reviewer.model = "deepseek-v4-pro".to_string();
        let toml = without_section(&config.to_toml_pretty().unwrap(), "[roles.reviewer]");

        let parsed = PureConfig::from_toml(&toml).unwrap();

        assert_eq!(parsed.roles.reviewer.provider, "deepseek");
        assert_eq!(parsed.roles.reviewer.model, "deepseek-v4-flash");
        assert_eq!(parsed.roles.reviewer.effort.as_str(), "high");
    }

    #[test]
    fn missing_all_roles_uses_first_provider_default_model() {
        let mut toml = PureConfig::default_config().to_toml_pretty().unwrap();
        for section in [
            "[roles.explorer]",
            "[roles.planner]",
            "[roles.executor]",
            "[roles.reviewer]",
        ] {
            toml = without_section(&toml, section);
        }

        let parsed = PureConfig::from_toml(&toml).unwrap();

        for role in ModelRole::all() {
            assert_eq!(parsed.role_config(role).provider, "deepseek");
            assert_eq!(parsed.role_config(role).model, "deepseek-v4-flash");
        }
    }

    #[test]
    fn complete_roles_do_not_require_default_model_effort_for_fallback() {
        let mut config = PureConfig::default_config();
        for role in ModelRole::all() {
            match role {
                ModelRole::Explorer => config.roles.explorer.model = "deepseek-v4-pro".to_string(),
                ModelRole::Planner => config.roles.planner.model = "deepseek-v4-pro".to_string(),
                ModelRole::Executor => config.roles.executor.model = "deepseek-v4-pro".to_string(),
                ModelRole::Reviewer => config.roles.reviewer.model = "deepseek-v4-pro".to_string(),
            }
        }
        config.providers.get_mut("deepseek").unwrap().models[0]
            .reasoning_efforts
            .clear();

        let parsed = PureConfig::from_toml(&config.to_toml_pretty().unwrap()).unwrap();

        assert_eq!(
            parsed.role_config(ModelRole::Planner).model,
            "deepseek-v4-pro"
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

    #[test]
    fn legacy_schema_is_backed_up_and_migrated() {
        let home = temp_home("legacy-migrate");
        let store = ConfigStore::new(ConfigPaths::from_home(&home));
        let mut legacy = PureConfig::default_config();
        legacy.schema_version = LEGACY_CONFIG_SCHEMA_VERSION;
        fs::create_dir_all(store.paths().config_dir()).unwrap();
        fs::write(
            store.paths().config_file(),
            legacy.to_toml_pretty().unwrap(),
        )
        .unwrap();

        let migrated = store.load_or_default().unwrap();
        let saved = store.load().unwrap();
        let backup_exists = fs::read_dir(store.paths().config_dir())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("config.v1.backup.")
            });

        assert_eq!(migrated.schema_version, CONFIG_SCHEMA_VERSION);
        assert_eq!(saved.schema_version, CONFIG_SCHEMA_VERSION);
        assert!(backup_exists);
        fs::remove_dir_all(home).unwrap();
    }
}
