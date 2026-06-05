mod provider;
mod role;
mod runtime;
mod store;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use pl_protocol::{PureError, Result};
use serde::{Deserialize, Serialize};

pub use provider::{ModelCapabilityConfig, ModelConfig, ProviderConfig, TruncationPolicyConfig};
pub use role::{ModelRole, ReasoningEffort, ResolvedRoleConfig, RoleConfig, RoleConfigs};
pub use runtime::{RuntimeConfig, SkillsConfig, SystemSkillsConfig};
pub use store::{ConfigPaths, ConfigStore};

pub const CONFIG_DIR_NAME: &str = ".pure";
pub const CONFIG_FILE_NAME: &str = "config.toml";
pub const CONFIG_SCHEMA_VERSION: u32 = 3;

pub(super) const DEFAULT_PROVIDER_KEY: &str = "deepseek";
pub(super) const DEFAULT_MODEL: &str = "deepseek-v4-flash";
const DEFAULT_EFFORT: &str = "high";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PureConfig {
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "RuntimeConfig::is_empty")]
    pub runtime: RuntimeConfig,
    #[serde(default, skip_serializing_if = "SkillsConfig::is_default")]
    pub skills: SkillsConfig,
    pub roles: RoleConfigs,
    pub providers: BTreeMap<String, ProviderConfig>,
}

#[derive(Debug, Clone, Deserialize)]
struct PureConfigToml {
    pub schema_version: u32,
    #[serde(default)]
    pub runtime: RuntimeConfig,
    #[serde(default)]
    pub skills: SkillsConfig,
    #[serde(default)]
    pub roles: Option<role::RoleConfigsToml>,
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
            runtime: RuntimeConfig::default(),
            skills: SkillsConfig::default(),
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

        crate::skill::validate_skills_config(&self.skills)?;

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
            None => RoleConfigs::from_default_role(role::default_role_config(&raw.providers)?),
        };
        let config = Self {
            schema_version: raw.schema_version,
            runtime: raw.runtime,
            skills: raw.skills,
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
