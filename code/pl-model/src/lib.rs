//! 模型目录、provider endpoint、canonical completion 与单模型运行时。

mod completion;
mod model;
mod provider;
mod runtime;

pub use completion::{
    ClickOperation, CompletionRequest, CompletionRequestBuilder, CompletionResponse,
    CompletionTraceContext, ExternalWebAccess, ExternalWebAccessMode, FinanceAssetType,
    FinanceOperation, FindOperation, InvalidToolArguments, ModelCompactionRequest,
    ModelCompactionResponse, OpenAiCompactionMode, OpenOperation, ReasoningConfig,
    ReasoningSummary, ScreenshotOperation, SearchAllowedCaller, SearchCommands, SearchQuery,
    SearchRequest, SearchResponse, SearchResponseLength, SearchSettings, SportsFunction,
    SportsLeague, SportsOperation, SportsToolName, TimeOperation, TokenUsage, ToolCall,
    ToolCallPayload, ToolCallerMode, ToolFormat, ToolSchema, WeatherOperation, WebSearchAction,
    WebSearchConfig, WebSearchContextSize, WebSearchFilters, WebSearchLocation, WebSearchMode,
    WebSearchUserLocation, WebSearchUserLocationType,
};
pub use model::{
    MaxTokensField, MissingCandidatePolicy, ModelCapabilities, ModelFamily, ModelInfo,
    ModelModality, ModelParameter, ModelParameterCandidateError, ModelParameterCandidateRequest,
    ModelPricing, ModelRequestProfile, ModelTransportProfile, ParameterWire,
    PromptCacheModelCapabilities, ProviderCapabilities, ReasoningInterleaved,
    ReasoningInterleavedField, ResponsesMaxTokensField, ToolCapabilities, TruncationMode,
    TruncationPolicy, WireAssignment, deepseek_default_model_slugs, default_models,
    mimo_default_model_slugs, openai_default_model_slugs, wire_assignments_from_value,
    zhipu_default_model_slugs,
};
pub use pl_protocol::ToolCallKind;
pub use provider::{
    ApplyPatchToolType, EffectivePromptCachePolicy, PromptCacheDialect,
    PromptCacheProviderCapabilities, ProviderConnectionMode, ProviderEndpoint,
    ProviderServiceCapabilities, ProviderWireProtocol, ResponsesHostedToolCapabilities,
    StandaloneWebSearchDialect, ToolWirePolicy, WebSearchProviderCapabilities,
    ZHIPU_CODING_PLAN_BASE_URL, provider_transport_profile_revision,
};
pub use runtime::{ModelInvocationContext, ModelRuntime, ModelSession};
