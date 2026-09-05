//! 模型元数据、能力、参数与内置数据目录。

pub(crate) mod capabilities;
mod catalog;
mod family;
pub(crate) mod info;
mod parameter;
mod pricing;
mod profile_error;

pub use capabilities::{
    ModelCapabilities, ModelInputCapability, ModelInputLimits, ModelInputSource, ModelModality,
    PromptCacheModelCapabilities, ReasoningInterleaved, ReasoningInterleavedField,
    ToolCapabilities,
};
pub use catalog::{
    deepseek_default_model_slugs, default_models, mimo_default_model_slugs,
    openai_default_model_slugs, zhipu_default_model_slugs,
};
pub use family::ModelFamily;
pub use info::{
    ChatRequestOptions, MaxTokensField, MediaMixPolicy, MediaRepresentation, MediaWireFormat,
    ModelBinding, ModelInfo, ModelMediaInputProfile, ModelProtocolOptions, ModelRequestProfile,
    ModelTransportProfile, ResponsesMaxTokensField, ResponsesRequestOptions, TruncationMode,
    TruncationPolicy,
};
pub use parameter::{
    MissingCandidatePolicy, ModelParameter, ModelParameterCandidateError,
    ModelParameterCandidateRequest, ParameterWire, WireAssignment, wire_assignments_from_value,
};
pub use pricing::{
    DailyPriceWindow, ModelPricing, PricingError, TokenPriceTier, WeeklyPriceAdjustment,
};

pub use pl_protocol::{
    InferenceAccounting, ModelPriceTierDto, ModelPricingDto, PricingMode, UsageReport,
};
