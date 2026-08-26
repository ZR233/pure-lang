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
    ToolCallPayload, WeatherOperation, WebSearchAction, WebSearchConfig, WebSearchContextSize,
    WebSearchFilters, WebSearchLocation, WebSearchMode, WebSearchUserLocation,
    WebSearchUserLocationType,
};
pub use model::{
    MaxTokensField, MissingCandidatePolicy, ModelCapabilities, ModelFamily, ModelInfo,
    ModelModality, ModelParameter, ModelParameterCandidateError, ModelParameterCandidateRequest,
    ModelPricing, ModelRequestProfile, ModelTransportProfile, ParameterWire,
    PromptCacheModelCapabilities, ReasoningInterleaved, ReasoningInterleavedField,
    ResponsesMaxTokensField, ToolCapabilities, TruncationMode, TruncationPolicy, WireAssignment,
    deepseek_default_model_slugs, default_models, mimo_default_model_slugs,
    openai_default_model_slugs, wire_assignments_from_value, zhipu_default_model_slugs,
};
pub use pl_protocol::{ToolCallKind, ToolCallerMode, ToolFormat, ToolSpec};
pub use provider::{
    ApplyPatchToolType, EffectivePromptCachePolicy, PromptCacheDialect,
    PromptCacheProviderCapabilities, ProviderConnectionMode, ProviderEndpoint,
    ProviderServiceCapabilities, ProviderWireProtocol, ResponsesHostedToolCapabilities,
    StandaloneWebSearchDialect, ToolWirePolicy, WebSearchProviderCapabilities,
    ZHIPU_CODING_PLAN_BASE_URL, provider_transport_profile_revision,
};
pub use runtime::{ModelInvocationContext, ModelRuntime, ModelSession};

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    fn rust_sources(root: &Path) -> Vec<PathBuf> {
        let mut pending = vec![root.to_path_buf()];
        let mut sources = Vec::new();
        while let Some(directory) = pending.pop() {
            for entry in std::fs::read_dir(directory).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    pending.push(path);
                } else if path.extension().is_some_and(|extension| extension == "rs") {
                    sources.push(path);
                }
            }
        }
        sources
    }

    #[test]
    fn crate_keeps_one_runtime_path_without_obsolete_model_layers() {
        let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut top_level = std::fs::read_dir(&source_root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        top_level.sort();
        assert_eq!(
            top_level,
            ["completion", "lib.rs", "model", "provider", "runtime"]
        );

        let production = ["completion", "model", "provider", "runtime"]
            .into_iter()
            .flat_map(|module| rust_sources(&source_root.join(module)))
            .map(|path| std::fs::read_to_string(path).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        for forbidden in [
            "trait ModelProvider",
            "SharedModelProvider",
            "OpenAiProvider",
            "ModelsManager",
            "create_provider",
            "decode_provider_stream",
            "#[path =",
        ] {
            assert!(
                !production.contains(forbidden),
                "obsolete model architecture symbol reappeared: {forbidden}"
            );
        }
    }
}
