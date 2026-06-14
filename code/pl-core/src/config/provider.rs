use std::collections::HashMap;

use pl_model::{
    ApplyPatchToolType, ModelInfo, ProviderInfo, ProviderKind, ToolWirePolicy,
    deepseek_default_model_slugs, default_models,
};
use pl_protocol::{PureError, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderConfig {
    pub provider_kind: ProviderKind,
    pub name: String,
    pub base_url: String,
    pub default_model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bearer_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_headers: Option<HashMap<String, String>>,
    #[serde(default)]
    pub tool_wire_policy: ToolWirePolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apply_patch_tool_type: Option<ApplyPatchToolType>,
    #[serde(default)]
    pub models: Vec<ModelInfo>,
}

impl ProviderConfig {
    pub fn default_deepseek() -> Self {
        let info = ProviderInfo::deepseek(None);
        let slugs = deepseek_default_model_slugs();
        let models = default_models()
            .into_iter()
            .filter(|model| slugs.contains(&model.slug.as_str()))
            .collect();
        Self::from_provider_info(info, models)
    }

    pub fn from_provider_info(info: ProviderInfo, models: Vec<ModelInfo>) -> Self {
        Self {
            provider_kind: info.provider_kind,
            name: info.name,
            base_url: info.base_url,
            default_model: info.default_model,
            bearer_token: info.bearer_token,
            http_headers: info.http_headers,
            tool_wire_policy: info.tool_wire_policy,
            apply_patch_tool_type: info.apply_patch_tool_type,
            models,
        }
    }

    pub fn to_provider_info(&self, default_model: &str) -> ProviderInfo {
        ProviderInfo {
            provider_kind: self.provider_kind,
            name: self.name.clone(),
            base_url: self.base_url.clone(),
            default_model: default_model.to_string(),
            bearer_token: self.bearer_token.clone(),
            http_headers: self.http_headers.clone(),
            tool_wire_policy: self.tool_wire_policy,
            apply_patch_tool_type: self.apply_patch_tool_type,
        }
    }

    pub(super) fn validate(&self, provider_key: &str) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(PureError::ConfigError(format!(
                "provider {provider_key} has empty name"
            )));
        }
        if self.base_url.trim().is_empty() {
            return Err(PureError::ConfigError(format!(
                "provider {provider_key} has empty base_url"
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
