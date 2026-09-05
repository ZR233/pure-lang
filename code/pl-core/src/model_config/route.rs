use std::collections::BTreeMap;

use pl_protocol::{PureError, Result};
use serde::{Deserialize, Serialize};

use super::{AgentRoleId, ProviderConfig, ProviderId};
use pl_model::model::ModelInfo;
use pl_model::provider::ProviderEndpoint;

/// 模型推理强度的产品无关字符串值。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct ReasoningEffort(String);

impl ReasoningEffort {
    /// 创建推理强度值；最终有效性由具体模型参数目录校验。
    pub fn new(effort: impl Into<String>) -> Self {
        Self(effort.into())
    }

    /// 返回配置文本。
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// 是否明确禁用推理。
    pub fn is_none(&self) -> bool {
        self.as_str() == "none"
    }
}

/// 动态 agent 角色到 provider/model 的唯一路由。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelRouteConfig {
    pub provider: ProviderId,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<ReasoningEffort>,
}

/// 可嵌入任意产品配置文档的模型配置。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AgentModelConfig {
    pub providers: BTreeMap<ProviderId, ProviderConfig>,
    pub routes: BTreeMap<AgentRoleId, ModelRouteConfig>,
}

/// 完成引用校验后可直接创建模型 provider 的角色路由。
#[derive(Debug, Clone)]
pub struct ResolvedModelRoute {
    pub pricing_mode: pl_protocol::PricingMode,
    pub role: AgentRoleId,
    pub provider_id: ProviderId,
    pub endpoint: ProviderEndpoint,
    pub model: ModelInfo,
    pub effort: Option<ReasoningEffort>,
}

impl AgentModelConfig {
    /// 校验所有 provider 与动态角色路由引用。
    pub fn validate(&self) -> Result<()> {
        if self.providers.is_empty() {
            return Err(PureError::ConfigError(
                "at least one provider is required".to_string(),
            ));
        }
        for (provider_id, provider) in &self.providers {
            provider.validate(provider_id)?;
        }
        for role in self.routes.keys() {
            self.resolve(role)?;
        }
        Ok(())
    }

    /// 解析一个动态角色，并返回选中的 provider、模型和推理强度。
    pub fn resolve(&self, role: &AgentRoleId) -> Result<ResolvedModelRoute> {
        let route = self.routes.get(role).ok_or_else(|| {
            PureError::ConfigError(format!("missing model route for role: {role}"))
        })?;
        if route.model.trim().is_empty() {
            return Err(PureError::ConfigError(format!(
                "role {role} has empty model"
            )));
        }
        let provider = self.providers.get(&route.provider).ok_or_else(|| {
            PureError::ConfigError(format!(
                "role {role} references missing provider: {}",
                route.provider
            ))
        })?;
        let models = provider.effective_models()?;
        let model = models
            .iter()
            .find(|model| model.slug == route.model)
            .cloned()
            .ok_or_else(|| {
                PureError::ConfigError(format!(
                    "role {role} references missing model: {}.{}",
                    route.provider, route.model
                ))
            })?;
        let candidates = model.supported_efforts();
        match (&route.effort, candidates.is_empty()) {
            (Some(_), true) => {
                return Err(PureError::ConfigError(format!(
                    "role {role} sets effort for model without an effort parameter: {}.{}",
                    route.provider, route.model
                )));
            }
            (None, false) => {
                return Err(PureError::ConfigError(format!(
                    "role {role} must select an effort for model {}.{}",
                    route.provider, route.model
                )));
            }
            (Some(effort), false)
                if !candidates
                    .iter()
                    .any(|candidate| candidate == effort.as_str()) =>
            {
                return Err(PureError::ConfigError(format!(
                    "role {role} uses unsupported effort '{}' for model {}.{}",
                    effort.as_str(),
                    route.provider,
                    route.model
                )));
            }
            _ => {}
        }
        Ok(ResolvedModelRoute {
            pricing_mode: provider.pricing_mode,
            role: role.clone(),
            provider_id: route.provider.clone(),
            endpoint: provider.to_endpoint()?,
            model,
            effort: route.effort.clone(),
        })
    }
}
