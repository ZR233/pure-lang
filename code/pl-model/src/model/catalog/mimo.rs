//! Xiaomi MiMo 内建模型目录（OpenAI-compatible Chat Completions）。

use crate::model::capabilities::{
    ModelCapabilities, ModelInputCapability, ModelInputSource, ModelModality,
    PromptCacheModelCapabilities, ReasoningInterleaved, ReasoningInterleavedField,
    ToolCapabilities,
};
use crate::model::family::{ModelFamily, ModelInstanceSpec, ModelPricing};
use crate::model::info::{
    MaxTokensField, MediaWireFormat, ModelInfo, ModelRequestProfile, ModelTransportProfile,
    TruncationMode,
};
use crate::model::parameter::ModelParameter;

const MIMO_DEFAULT_MODEL_SLUGS: &[&str] =
    &["mimo-v2.5-pro", "mimo-v2.5", "mimo-v2-pro", "mimo-v2-omni"];

pub fn mimo_default_model_slugs() -> &'static [&'static str] {
    MIMO_DEFAULT_MODEL_SLUGS
}

pub(super) fn models() -> Vec<ModelInfo> {
    let mimo_text = mimo_text_family();
    let mimo_vision = mimo_vision_family();
    vec![
        mimo_text.instantiate(ModelInstanceSpec {
            slug: "mimo-v2.5-pro",
            display_name: "MiMo V2.5 Pro",
            description: "Xiaomi MiMo flagship model for long-horizon agent work.",
            context_window: 1_000_000,
            max_context_window: 1_000_000,
            max_output_tokens: Some(131_072),
            pricing: ModelPricing::default(),
        }),
        mimo_vision.instantiate(ModelInstanceSpec {
            slug: "mimo-v2.5",
            display_name: "MiMo V2.5",
            description:
                "Xiaomi MiMo full-modal agent model with a one-million-token context window.",
            context_window: 1_000_000,
            max_context_window: 1_000_000,
            max_output_tokens: Some(32_768),
            pricing: ModelPricing::default(),
        }),
        mimo_text.instantiate(ModelInstanceSpec {
            slug: "mimo-v2-pro",
            display_name: "MiMo V2 Pro",
            description: "Xiaomi MiMo long-context reasoning model for complex agent tasks.",
            context_window: 1_000_000,
            max_context_window: 1_000_000,
            max_output_tokens: Some(131_072),
            pricing: ModelPricing::default(),
        }),
        mimo_vision.instantiate(ModelInstanceSpec {
            slug: "mimo-v2-omni",
            display_name: "MiMo V2 Omni",
            description: "Xiaomi MiMo multimodal model for text and visual agent tasks.",
            context_window: 256_000,
            max_context_window: 256_000,
            max_output_tokens: Some(32_768),
            pricing: ModelPricing::default(),
        }),
    ]
}

fn mimo_text_family() -> ModelFamily {
    mimo_family("mimo-text", mimo_text_capabilities(), Vec::new())
}

fn mimo_vision_family() -> ModelFamily {
    mimo_family(
        "mimo-vision",
        mimo_vision_capabilities(),
        super::image_media_profiles(MediaWireFormat::ChatImageUrl, false),
    )
}

/// MiMo 共享 family 元数据；视觉变体叠加本地图片输入能力。
fn mimo_family(
    id: &'static str,
    capabilities: ModelCapabilities,
    media: Vec<super::ModelMediaInputProfile>,
) -> ModelFamily {
    ModelFamily {
        id,
        capabilities,
        truncation_mode: TruncationMode::Tokens,
        truncation_limit: 10_000,
        parameters: vec![mimo_effort_parameter()],
        transport: ModelTransportProfile::chat_completions_http(),
        request_profile: ModelRequestProfile {
            max_tokens_field: MaxTokensField::MaxCompletionTokens,
            media,
            ..ModelRequestProfile::default()
        },
        base_instructions: String::new(),
    }
}

/// MiMo thinking 开关；官方 Chat API 只声明 `thinking.type`。
fn mimo_effort_parameter() -> ModelParameter {
    ModelParameter {
        name: "effort".to_string(),
        label: Some("Thinking".to_string()),
        candidates: vec!["disabled".to_string(), "enabled".to_string()],
        wire: ["disabled", "enabled"]
            .into_iter()
            .map(|value| {
                (
                    value.to_string(),
                    super::wire_set_one("thinking.type", value),
                )
            })
            .collect(),
    }
}

fn mimo_text_capabilities() -> ModelCapabilities {
    mimo_capabilities(vec![ModelInputCapability::text()])
}

fn mimo_vision_capabilities() -> ModelCapabilities {
    mimo_capabilities(vec![
        ModelInputCapability::text(),
        ModelInputCapability::media(ModelModality::Image, vec![ModelInputSource::Local]),
    ])
}

fn mimo_capabilities(input: Vec<ModelInputCapability>) -> ModelCapabilities {
    ModelCapabilities {
        streaming: true,
        temperature: false,
        reasoning: true,
        web_search: false,
        input,
        output: vec![ModelModality::Text],
        tools: ToolCapabilities {
            function_calling: true,
            parallel_tool_calls: false,
            custom_tools: false,
            freeform_tools: false,
            programmatic_tool_calling: false,
        },
        interleaved: Some(ReasoningInterleaved {
            field: ReasoningInterleavedField::ReasoningContent,
        }),
        prompt_cache: PromptCacheModelCapabilities::default(),
    }
}
