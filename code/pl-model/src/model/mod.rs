//! 模型元数据、能力、参数与内置数据目录。

pub(crate) mod capabilities;
mod catalog;
mod definition;
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
pub(crate) use catalog::zhipu_responses_models;
pub(crate) use catalog::{MediaSendOrder, image_media_profiles};
pub use catalog::{
    default_models, mimo_default_model_slugs, zhipu_default_model_slugs,
    zhipu_responses_default_model_slugs,
};
pub(crate) use definition::validate_inventory;
pub use definition::{BundledModelDefinition, ModelDefinitionError, bundled_model_definition};
pub use family::ModelFamily;
pub use info::{
    ChatRequestOptions, DEFAULT_AUTO_COMPACT_TOKEN_LIMIT, MaxTokensField, MediaMixPolicy,
    MediaRepresentation, MediaWireFormat, ModelBinding, ModelInfo, ModelMediaInputProfile,
    ModelProtocolOptions, ModelRequestProfile, ModelTransportProfile, ResponsesMaxTokensField,
    ResponsesRequestOptions, TruncationMode, TruncationPolicy,
};
pub use parameter::{
    MissingCandidatePolicy, ModelParameter, ModelParameterCandidateError,
    ModelParameterCandidateRequest, ParameterWire, WireAssignment, wire_assignments_from_value,
};
pub use pricing::{
    DailyPriceWindow, ModelPricing, PricingError, TokenPriceTier, WeeklyPriceAdjustment,
};
pub use profile_error::ModelProfileError;

pub use pl_protocol::{
    InferenceAccounting, ModelPriceTierDto, ModelPricingDto, PricingMode, UsageReport,
};
