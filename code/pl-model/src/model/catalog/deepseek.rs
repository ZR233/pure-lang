//! DeepSeek 内建模型目录：共享 reasoning family，Flash 与 Pro 路由到 Responses HTTP。

use crate::model::pricing::{ModelPricing, TokenPriceTier};
use serde_json::{Map, Value};

use crate::model::capabilities::{
    ModelCapabilities, ModelInputCapability, ModelInputLimits, ModelInputSource, ModelModality,
    PromptCacheModelCapabilities, ReasoningInterleaved, ReasoningInterleavedField,
    ToolCapabilities,
};
use crate::model::family::{ModelFamily, ModelInstanceSpec};
use crate::model::info::{
    MediaWireFormat, ModelInfo, ModelRequestProfile, ModelTransportProfile, TruncationMode,
};
use crate::model::parameter::ModelParameter;

const DEEPSEEK_DEFAULT_MODEL_SLUGS: &[&str] = &[
    "deepseek-v4-flash",
    "deepseek-v4-flash-vision-exp",
    "deepseek-v4-pro",
];

pub fn deepseek_default_model_slugs() -> &'static [&'static str] {
    DEEPSEEK_DEFAULT_MODEL_SLUGS
}

pub(super) fn models() -> Vec<ModelInfo> {
    vec![
        deepseek_family().instantiate(ModelInstanceSpec {
            slug: "deepseek-v4-flash",
            display_name: "DeepSeek V4 Flash",
            description: "DeepSeek fast reasoning model with thinking mode.",
            context_window: 1_000_000,
            max_context_window: 1_000_000,
            max_output_tokens: Some(384_000),
            pricing: deepseek_pricing(1.5, 4.5, 0.05),
        }),
        deepseek_vision_family().instantiate(ModelInstanceSpec {
            slug: "deepseek-v4-flash-vision-exp",
            display_name: "DeepSeek V4 Flash Vision Exp",
            description:
                "DeepSeek experimental multimodal reasoning model with image understanding.",
            context_window: 1_000_000,
            max_context_window: 1_000_000,
            max_output_tokens: Some(384_000),
            pricing: deepseek_pricing(1.5, 4.5, 0.05),
        }),
        deepseek_family().instantiate(ModelInstanceSpec {
            slug: "deepseek-v4-pro",
            display_name: "DeepSeek V4 Pro",
            description: "DeepSeek flagship reasoning model with thinking mode.",
            context_window: 1_000_000,
            max_context_window: 1_000_000,
            max_output_tokens: Some(384_000),
            pricing: deepseek_pricing(4.5, 13.5, 0.15),
        }),
    ]
}

/// DeepSeek 内建模型共享的 family 元数据；当前全部路由到 Responses HTTP。
fn deepseek_family() -> ModelFamily {
    ModelFamily {
        id: "deepseek-reasoning",
        capabilities: deepseek_capabilities(),
        truncation_mode: TruncationMode::Tokens,
        truncation_limit: 10_000,
        parameters: vec![deepseek_effort_parameter()],
        transport: ModelTransportProfile::responses_http(),
        request_profile: deepseek_request_profile(),
        base_instructions: String::new(),
    }
}

fn deepseek_vision_family() -> ModelFamily {
    ModelFamily {
        id: "deepseek-vision-reasoning",
        capabilities: deepseek_vision_capabilities(),
        truncation_mode: TruncationMode::Tokens,
        truncation_limit: 10_000,
        parameters: vec![deepseek_effort_parameter()],
        transport: ModelTransportProfile::responses_http(),
        request_profile: deepseek_request_profile().with_image_media(
            MediaWireFormat::ResponsesInputImage,
            super::MediaSendOrder::RemoteUrlFirst,
        ),
        base_instructions: String::new(),
    }
}

/// DeepSeek 固定 base body：`thinking.type = enabled`（DeepSeek 模型始终开启 thinking）。
fn deepseek_request_profile() -> ModelRequestProfile {
    let mut thinking = Map::new();
    thinking.insert("type".to_string(), Value::String("enabled".to_string()));
    let mut body = Map::new();
    body.insert("thinking".to_string(), Value::Object(thinking));
    ModelRequestProfile {
        body,
        ..ModelRequestProfile::responses()
    }
}

/// DeepSeek effort：候选值按弱到强 high/max，透传到 `reasoning_effort`。
fn deepseek_effort_parameter() -> ModelParameter {
    ModelParameter {
        name: "effort".to_string(),
        label: None,
        candidates: vec!["high".to_string(), "max".to_string()],
        wire: ["high", "max"]
            .into_iter()
            .map(|value| {
                (
                    value.to_string(),
                    super::wire_set_one("reasoning_effort", value),
                )
            })
            .collect(),
    }
}

fn deepseek_capabilities() -> ModelCapabilities {
    ModelCapabilities {
        streaming: true,
        temperature: false,
        reasoning: true,
        web_search: true,
        input: vec![ModelInputCapability::text()],
        output: vec![ModelModality::Text],
        tools: ToolCapabilities {
            function_calling: true,
            parallel_tool_calls: true,
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

fn deepseek_vision_capabilities() -> ModelCapabilities {
    let mut capabilities = deepseek_capabilities();
    capabilities.input.push(ModelInputCapability {
        modality: ModelModality::Image,
        sources: vec![ModelInputSource::Local, ModelInputSource::RemoteUrl],
        limits: ModelInputLimits {
            max_count: Some(600),
            max_bytes: Some(32 * 1024 * 1024),
            max_total_bytes: Some(32 * 1024 * 1024),
            max_width: Some(4096),
            max_height: Some(4096),
            media_types: ["image/jpeg", "image/png", "image/gif", "image/webp"]
                .into_iter()
                .map(ToString::to_string)
                .collect(),
        },
    });
    capabilities
}

fn deepseek_pricing(input: f64, output: f64, read: f64) -> ModelPricing {
    use crate::model::pricing::{DailyPriceWindow, WeeklyPriceAdjustment};
    ModelPricing::published(
        "CNY",
        vec![TokenPriceTier::flat(input, output, Some(read), None)],
        "https://api-docs.deepseek.com/zh-cn/quick_start/pricing/",
    )
    .with_weekly_adjustment(WeeklyPriceAdjustment {
        utc_offset_minutes: 480,
        weekdays: vec![1, 2, 3, 4, 5],
        windows: vec![
            DailyPriceWindow {
                start_minute: 540,
                end_minute: 720,
            },
            DailyPriceWindow {
                start_minute: 840,
                end_minute: 1080,
            },
        ],
        multiplier: 2.0,
    })
}
