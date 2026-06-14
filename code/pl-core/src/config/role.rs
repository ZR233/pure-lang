use std::collections::BTreeMap;

use pl_model::{ModelInfo, ProviderInfo};
use pl_protocol::{PureError, Result};
use serde::{Deserialize, Serialize};

use super::provider::ProviderConfig;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RoleConfig {
    pub provider: String,
    pub model: String,
    pub effort: ReasoningEffort,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RoleConfigs {
    pub explorer: RoleConfig,
    pub planner: RoleConfig,
    pub executor: RoleConfig,
    pub reviewer: RoleConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct RoleConfigsToml {
    #[serde(default)]
    pub explorer: Option<RoleConfig>,
    #[serde(default)]
    pub planner: Option<RoleConfig>,
    #[serde(default)]
    pub executor: Option<RoleConfig>,
    #[serde(default)]
    pub reviewer: Option<RoleConfig>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct ReasoningEffort(String);

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
    pub(super) fn into_role_configs(
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

pub(super) fn default_role_config(
    providers: &BTreeMap<String, ProviderConfig>,
) -> Result<RoleConfig> {
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
    let effort = model.default_effort().ok_or_else(|| {
        PureError::ConfigError(format!(
            "default model {} must define effort parameter",
            provider.default_model
        ))
    })?;

    Ok(RoleConfig {
        provider: provider_key.clone(),
        model: provider.default_model.clone(),
        effort: ReasoningEffort::new(effort),
    })
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
