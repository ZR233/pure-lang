//! 模型家族预设，封装同 provider 模型共享的元数据。
//!
//! 参见 design/07-model.md 7.9 节。同 provider 的模型共享 capabilities、
//! truncation_policy、effort 参数声明和 base body。具体模型实例通过差异字段
//! （slug、display_name、context_window 等）从 family 派生，避免在
//! `default_models` 中为每个模型重复构造完整 `ModelInfo`。

use crate::model::capabilities::ModelCapabilities;
use crate::model::info::{
    ModelInfo, ModelRequestProfile, ModelTransportProfile, TruncationMode, TruncationPolicy,
};
use crate::model::parameter::ModelParameter;
/// 同一 provider 内共享元数据的模型家族预设。
///
/// 封装同 provider 模型共享的 capabilities、truncation_policy、parameters、
/// request_profile 等。具体模型实例通过 [`instantiate`](Self::instantiate)
/// 用差异字段派生。
#[derive(Debug, Clone)]
pub struct ModelFamily {
    /// 家族标识（调试用，如 "openai-reasoning"、"zhipu-text"）。
    pub id: &'static str,
    pub capabilities: ModelCapabilities,
    pub truncation_mode: TruncationMode,
    pub truncation_limit: u64,
    /// 共享的可调参数声明（如 effort）。
    pub parameters: Vec<ModelParameter>,
    pub transport: ModelTransportProfile,
    /// 共享的请求 profile（含 base body，如 DeepSeek 的 `thinking.type = enabled`）。
    pub request_profile: ModelRequestProfile,
    pub base_instructions: String,
}

/// 模型定价。各字段缺失表示不计价。
#[derive(Debug, Clone, Default)]
pub struct ModelPricing {
    pub currency: Option<String>,
    pub input_per_mtok: Option<f64>,
    pub output_per_mtok: Option<f64>,
    pub cache_read_per_mtok: Option<f64>,
    pub cache_write_per_mtok: Option<f64>,
}

/// 家族内单个具体模型的差异字段，[`ModelFamily::instantiate`] 的输入。
#[derive(Debug, Clone)]
pub struct ModelInstanceSpec {
    pub slug: &'static str,
    pub display_name: &'static str,
    pub description: &'static str,
    pub context_window: u64,
    pub max_context_window: u64,
    pub max_output_tokens: Option<u64>,
    pub pricing: ModelPricing,
}

impl ModelFamily {
    /// 用差异字段实例化一个具体 `ModelInfo`。
    pub fn instantiate(&self, spec: ModelInstanceSpec) -> ModelInfo {
        let ModelInstanceSpec {
            slug,
            display_name,
            description,
            context_window,
            max_context_window,
            max_output_tokens,
            pricing:
                ModelPricing {
                    currency,
                    input_per_mtok,
                    output_per_mtok,
                    cache_read_per_mtok,
                    cache_write_per_mtok,
                },
        } = spec;
        ModelInfo {
            slug: slug.to_string(),
            display_name: display_name.to_string(),
            description: Some(description.to_string()),
            context_window: Some(context_window),
            max_context_window: Some(max_context_window),
            auto_compact_token_limit: None,
            default_temperature: None,
            max_output_tokens,
            currency,
            input_price_per_mtok: input_per_mtok,
            output_price_per_mtok: output_per_mtok,
            cache_read_price_per_mtok: cache_read_per_mtok,
            cache_write_price_per_mtok: cache_write_per_mtok,
            parameters: self.parameters.clone(),
            transport: self.transport.clone(),
            capabilities: self.capabilities.clone(),
            request_profile: self.request_profile.clone(),
            truncation_policy: TruncationPolicy {
                mode: self.truncation_mode,
                limit: self.truncation_limit,
            },
            base_instructions: self.base_instructions.clone(),
            used_fallback: false,
        }
    }
}
