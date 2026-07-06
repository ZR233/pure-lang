mod instruction;
mod mcp;
mod provider;
mod role;
mod runtime;
mod store;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use pl_protocol::{PureError, Result};
use serde::{Deserialize, Serialize};

pub use instruction::{DEFAULT_PROJECT_DOC_MAX_BYTES, InstructionsConfig};
pub use mcp::{
    BuiltinMcpServerState, EffectiveMcpServerConfig, McpServerConfig, McpServerMutationPolicy,
    McpServerSourceKind, McpServerStatusKind, McpServerTransport, active_mcp_server_names,
    builtin_mcp_server_ids, effective_mcp_servers, is_builtin_mcp_server_id,
    normalize_builtin_mcp_server_states, validate_mcp_identifier, zhipu_coding_plan_token,
};
pub use provider::ProviderConfig;
pub use role::{ModelRole, ReasoningEffort, ResolvedRoleConfig, RoleConfig, RoleConfigs};
pub use runtime::{RuntimeConfig, SkillsConfig, SystemSkillsConfig, ToolCapabilityConfig};
pub use store::{ConfigPaths, ConfigStore};

pub const CONFIG_DIR_NAME: &str = ".pure";
pub const CONFIG_FILE_NAME: &str = "config.toml";
pub const CONFIG_SCHEMA_VERSION: u32 = 4;

const DEFAULT_PROVIDER_KEY: &str = "deepseek";
pub(super) const DEFAULT_MODEL: &str = "deepseek-v4-flash";
const DEFAULT_EFFORT: &str = "high";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PureConfig {
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "RuntimeConfig::is_empty")]
    pub runtime: RuntimeConfig,
    #[serde(default, skip_serializing_if = "InstructionsConfig::is_default")]
    pub instructions: InstructionsConfig,
    #[serde(default, skip_serializing_if = "SkillsConfig::is_default")]
    pub skills: SkillsConfig,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub mcp_servers: BTreeMap<String, McpServerConfig>,
    #[serde(
        default,
        skip_serializing_if = "mcp::builtin_mcp_server_states_are_default"
    )]
    pub builtin_mcp_servers: BTreeMap<String, BuiltinMcpServerState>,
    pub roles: RoleConfigs,
    pub providers: BTreeMap<String, ProviderConfig>,
}

#[derive(Debug, Clone, Deserialize)]
struct PureConfigToml {
    pub schema_version: u32,
    #[serde(default)]
    pub runtime: RuntimeConfig,
    #[serde(default)]
    pub instructions: InstructionsConfig,
    #[serde(default)]
    pub skills: SkillsConfig,
    #[serde(default)]
    pub mcp_servers: BTreeMap<String, McpServerConfig>,
    #[serde(default)]
    pub builtin_mcp_servers: BTreeMap<String, BuiltinMcpServerState>,
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
            instructions: InstructionsConfig::default(),
            skills: SkillsConfig::default(),
            mcp_servers: BTreeMap::new(),
            builtin_mcp_servers: BTreeMap::new(),
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
        mcp::validate_mcp_servers(&self.mcp_servers)?;
        mcp::validate_builtin_mcp_server_states(&self.builtin_mcp_servers)?;

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
            .supported_efforts()
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
            models: provider.models.clone(),
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
        let mut config = Self {
            schema_version: raw.schema_version,
            runtime: raw.runtime,
            instructions: raw.instructions,
            skills: raw.skills,
            mcp_servers: raw.mcp_servers,
            builtin_mcp_servers: raw.builtin_mcp_servers,
            roles,
            providers: raw.providers,
        };
        mcp::normalize_builtin_mcp_server_states(&mut config);
        config.validate()?;
        Ok(config)
    }
}

impl Default for PureConfig {
    fn default() -> Self {
        Self::default_config()
    }
}
