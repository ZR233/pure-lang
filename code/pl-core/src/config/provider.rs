use std::collections::HashMap;

use pl_model::{
    ApplyPatchToolType, InputModality, ModelCapabilities, ModelInfo, ProviderInfo, ProviderKind,
    ToolWirePolicy, TruncationMode, TruncationPolicy, deepseek_default_model_slugs, default_models,
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
    pub models: Vec<ModelConfig>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TruncationPolicyConfig {
    pub mode: TruncationMode,
    pub limit: u64,
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

impl TruncationPolicyConfig {
    fn from_policy(policy: TruncationPolicy) -> Self {
        Self {
            mode: policy.mode,
            limit: policy.limit,
        }
    }

    pub(super) fn into_policy(self) -> TruncationPolicy {
        TruncationPolicy {
            mode: self.mode,
            limit: self.limit,
        }
    }
}
