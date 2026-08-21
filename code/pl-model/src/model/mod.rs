//! 模型元数据、能力、参数与内置数据目录。

pub(crate) mod capabilities;
mod catalog;
mod family;
pub(crate) mod info;
mod parameter;

pub use capabilities::{
    ModelCapabilities, ModelModality, PromptCacheModelCapabilities, ReasoningInterleaved,
    ReasoningInterleavedField, ToolCapabilities,
};
pub use catalog::{
    deepseek_default_model_slugs, default_models, mimo_default_model_slugs,
    openai_default_model_slugs, zhipu_default_model_slugs,
};
pub use family::{ModelFamily, ModelPricing};
pub use info::{
    MaxTokensField, ModelInfo, ModelRequestProfile, ModelTransportProfile, ResponsesMaxTokensField,
    TruncationMode, TruncationPolicy,
};
pub use parameter::{
    MissingCandidatePolicy, ModelParameter, ModelParameterCandidateError,
    ModelParameterCandidateRequest, ParameterWire, WireAssignment, wire_assignments_from_value,
};
