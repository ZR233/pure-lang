//! Zhipu GLM 内建模型目录（Chat Completions HTTP，effort 联动 thinking wire）。

use std::collections::BTreeMap;

use serde_json::Value;

use crate::model::capabilities::{
    ModelCapabilities, ModelInputCapability, ModelInputSource, ModelModality,
    PromptCacheModelCapabilities, ReasoningInterleaved, ReasoningInterleavedField,
    ToolCapabilities,
};
use crate::model::family::{ModelFamily, ModelInstanceSpec, ModelPricing};
use crate::model::info::{
    MediaWireFormat, ModelInfo, ModelRequestProfile, ModelTransportProfile, TruncationMode,
};
use crate::model::parameter::{ModelParameter, ParameterWire, WireAssignment};

const ZHIPU_GLM_DEFAULT_MODEL_SLUGS: &[&str] = &[
    "glm-5.3",
    "glm-5.3-flash",
    "glm-5.2",
    "glm-5",
    "glm-5-turbo",
    "glm-4.7",
    "glm-4.7-flashx",
    "glm-4.7-flash",
];

pub fn zhipu_default_model_slugs() -> &'static [&'static str] {
    ZHIPU_GLM_DEFAULT_MODEL_SLUGS
}

pub(super) fn models() -> Vec<ModelInfo> {
    let zhipu_text = zhipu_text_family();
    let zhipu_glm53 = zhipu_glm53_family();
    let zhipu_glm53_flash = zhipu_glm53_flash_family();
    let zhipu_glm52 = zhipu_glm52_family();
    let zhipu_vision = zhipu_vision_family();
    vec![
        // GLM-5.3（始终思考，effort 候选值按弱到强 low/high/max，联动 reasoning_effort + thinking）
        zhipu_glm53.instantiate(ModelInstanceSpec {
            slug: "glm-5.3",
            display_name: "GLM-5.3",
            description:
                "Zhipu flagship model for complex coding and agent work with always-on thinking.",
            context_window: 1_000_000,
            max_context_window: 1_000_000,
            max_output_tokens: Some(128_000),
            pricing: ModelPricing::default(),
        }),
        // GLM-5.3-Flash（GLM-5.3 同款始终思考 wire 的原生多模态版本，支持图片输入）
        zhipu_glm53_flash.instantiate(ModelInstanceSpec {
            slug: "glm-5.3-flash",
            display_name: "GLM-5.3-Flash",
            description:
                "Zhipu native multimodal coding model with always-on thinking and image input.",
            context_window: 1_000_000,
            max_context_window: 1_000_000,
            max_output_tokens: Some(128_000),
            pricing: ModelPricing::default(),
        }),
        // GLM-5.2（effort 候选值按弱到强 none/high/max，联动 reasoning_effort + thinking）
        zhipu_glm52.instantiate(ModelInstanceSpec {
            slug: "glm-5.2",
            display_name: "GLM-5.2",
            description: "Zhipu latest flagship model with stronger coding and long-horizon agent work.",
            context_window: 1_000_000,
            max_context_window: 1_000_000,
            max_output_tokens: Some(128_000),
            pricing: ModelPricing::default(),
        }),
        // 文本模型（effort 候选值按弱到强 none/enabled，映射 thinking 开关）
        zhipu_text.instantiate(ModelInstanceSpec {
            slug: "glm-5",
            display_name: "GLM-5",
            description: "Zhipu high-intelligence foundation model for coding and agentic planning.",
            context_window: 200_000,
            max_context_window: 200_000,
            max_output_tokens: Some(128_000),
            pricing: ModelPricing::default(),
        }),
        zhipu_text.instantiate(ModelInstanceSpec {
            slug: "glm-5-turbo",
            display_name: "GLM-5-Turbo",
            description: "Zhipu GLM-5 Turbo model optimized for long task continuity.",
            context_window: 200_000,
            max_context_window: 200_000,
            max_output_tokens: Some(128_000),
            pricing: ModelPricing::default(),
        }),
        zhipu_text.instantiate(ModelInstanceSpec {
            slug: "glm-4.7",
            display_name: "GLM-4.7",
            description: "Zhipu high-intelligence model for dialogue, reasoning, agents, and coding.",
            context_window: 200_000,
            max_context_window: 200_000,
            max_output_tokens: Some(128_000),
            pricing: ModelPricing::default(),
        }),
        zhipu_text.instantiate(ModelInstanceSpec {
            slug: "glm-4.7-flashx",
            display_name: "GLM-4.7-FlashX",
            description: "Zhipu lightweight high-speed model for general text tasks.",
            context_window: 200_000,
            max_context_window: 200_000,
            max_output_tokens: Some(128_000),
            pricing: ModelPricing::default(),
        }),
        zhipu_text.instantiate(ModelInstanceSpec {
            slug: "glm-4.6",
            display_name: "GLM-4.6",
            description: "Zhipu model with stronger coding, reasoning, and tool calling.",
            context_window: 200_000,
            max_context_window: 200_000,
            max_output_tokens: Some(128_000),
            pricing: ModelPricing::default(),
        }),
        zhipu_text.instantiate(ModelInstanceSpec {
            slug: "glm-4.5-air",
            display_name: "GLM-4.5-Air",
            description: "Zhipu cost-effective model for reasoning, coding, and agent tasks.",
            context_window: 128_000,
            max_context_window: 128_000,
            max_output_tokens: Some(96_000),
            pricing: ModelPricing::default(),
        }),
        zhipu_text.instantiate(ModelInstanceSpec {
            slug: "glm-4.5-airx",
            display_name: "GLM-4.5-AirX",
            description: "Zhipu fast cost-effective model for latency-sensitive tasks.",
            context_window: 128_000,
            max_context_window: 128_000,
            max_output_tokens: Some(96_000),
            pricing: ModelPricing::default(),
        }),
        zhipu_text.instantiate(ModelInstanceSpec {
            slug: "glm-4-long",
            display_name: "GLM-4-Long",
            description: "Zhipu long-context model for very large inputs and memory-heavy tasks.",
            context_window: 1_000_000,
            max_context_window: 1_000_000,
            max_output_tokens: Some(4_000),
            pricing: ModelPricing::default(),
        }),
        zhipu_text.instantiate(ModelInstanceSpec {
            slug: "glm-4-flashx-250414",
            display_name: "GLM-4-FlashX-250414",
            description: "Zhipu high-speed low-cost Flash model with higher concurrency.",
            context_window: 128_000,
            max_context_window: 128_000,
            max_output_tokens: Some(16_000),
            pricing: ModelPricing::default(),
        }),
        zhipu_text.instantiate(ModelInstanceSpec {
            slug: "glm-4.7-flash",
            display_name: "GLM-4.7-Flash",
            description: "Zhipu free GLM-4.7 base model.",
            context_window: 200_000,
            max_context_window: 200_000,
            max_output_tokens: Some(128_000),
            pricing: ModelPricing::default(),
        }),
        zhipu_text.instantiate(ModelInstanceSpec {
            slug: "glm-4.5-flash",
            display_name: "GLM-4.5-Flash",
            description: "Zhipu free GLM-4.5 model, marked by the official docs as being phased out.",
            context_window: 128_000,
            max_context_window: 128_000,
            max_output_tokens: Some(96_000),
            pricing: ModelPricing::default(),
        }),
        zhipu_text.instantiate(ModelInstanceSpec {
            slug: "glm-4-flash-250414",
            display_name: "GLM-4-Flash-250414",
            description: "Zhipu free GLM-4 Flash model for long-context multilingual tool use.",
            context_window: 128_000,
            max_context_window: 128_000,
            max_output_tokens: Some(16_000),
            pricing: ModelPricing::default(),
        }),
        // 视觉模型
        zhipu_vision.instantiate(ModelInstanceSpec {
            slug: "glm-5v-turbo",
            display_name: "GLM-5V-Turbo",
            description:
                "Zhipu multimodal coding model for visual understanding and agent workflows.",
            context_window: 200_000,
            max_context_window: 200_000,
            max_output_tokens: Some(128_000),
            pricing: ModelPricing::default(),
        }),
        zhipu_vision.instantiate(ModelInstanceSpec {
            slug: "glm-4.6v",
            display_name: "GLM-4.6V",
            description: "Zhipu visual reasoning model with native tool calling.",
            context_window: 128_000,
            max_context_window: 128_000,
            max_output_tokens: Some(32_000),
            pricing: ModelPricing::default(),
        }),
        zhipu_vision.instantiate(ModelInstanceSpec {
            slug: "glm-4.1v-thinking-flashx",
            display_name: "GLM-4.1V-Thinking-FlashX",
            description: "Zhipu lightweight visual reasoning model for complex scene understanding.",
            context_window: 64_000,
            max_context_window: 64_000,
            max_output_tokens: Some(16_000),
            pricing: ModelPricing::default(),
        }),
        zhipu_vision.instantiate(ModelInstanceSpec {
            slug: "glm-4.6v-flash",
            display_name: "GLM-4.6V-Flash",
            description: "Zhipu free visual reasoning model with tool calling.",
            context_window: 128_000,
            max_context_window: 128_000,
            max_output_tokens: Some(32_000),
            pricing: ModelPricing::default(),
        }),
        zhipu_vision.instantiate(ModelInstanceSpec {
            slug: "glm-4.1v-thinking-flash",
            display_name: "GLM-4.1V-Thinking-Flash",
            description: "Zhipu free visual reasoning model for multi-step analysis.",
            context_window: 64_000,
            max_context_window: 64_000,
            max_output_tokens: Some(16_000),
            pricing: ModelPricing::default(),
        }),
        zhipu_vision.instantiate(ModelInstanceSpec {
            slug: "glm-4v-flash",
            display_name: "GLM-4V-Flash",
            description: "Zhipu free image understanding model with multilingual support.",
            context_window: 16_000,
            max_context_window: 16_000,
            max_output_tokens: Some(1_000),
            pricing: ModelPricing::default(),
        }),
    ]
}

// ---- 家族预设 ----

fn zhipu_text_family() -> ModelFamily {
    zhipu_chat_family(
        "zhipu-text",
        zhipu_text_capabilities(),
        zhipu_plain_effort_parameter(),
    )
}

fn zhipu_glm52_family() -> ModelFamily {
    zhipu_chat_family(
        "zhipu-glm52",
        zhipu_text_capabilities(),
        zhipu_glm52_effort_parameter(),
    )
}

fn zhipu_glm53_family() -> ModelFamily {
    zhipu_chat_family(
        "zhipu-glm53",
        zhipu_text_capabilities(),
        zhipu_glm53_effort_parameter(),
    )
}

/// GLM-5.3-Flash 与 GLM-5.3 共用 effort wire，但官方归类为原生多模态模型。
fn zhipu_glm53_flash_family() -> ModelFamily {
    let mut family = zhipu_chat_family(
        "zhipu-glm53-flash",
        zhipu_vision_capabilities(),
        zhipu_glm53_effort_parameter(),
    );
    family.request_profile = family
        .request_profile
        .with_image_media(MediaWireFormat::ChatImageUrl, true);
    family
}

fn zhipu_vision_family() -> ModelFamily {
    let mut family = zhipu_chat_family(
        "zhipu-vision",
        zhipu_vision_capabilities(),
        zhipu_plain_effort_parameter(),
    );
    family.request_profile = family
        .request_profile
        .with_image_media(MediaWireFormat::ChatImageUrl, true);
    family
}

/// Zhipu Chat 家族共享骨架：Chat Completions HTTP + parallel tool calls base profile。
fn zhipu_chat_family(
    id: &'static str,
    capabilities: ModelCapabilities,
    effort_parameter: ModelParameter,
) -> ModelFamily {
    ModelFamily {
        id,
        capabilities,
        truncation_mode: TruncationMode::Tokens,
        truncation_limit: 10_000,
        parameters: vec![effort_parameter],
        transport: ModelTransportProfile::chat_completions_http(),
        request_profile: chat_parallel_request_profile(),
        base_instructions: String::new(),
    }
}

fn chat_parallel_request_profile() -> ModelRequestProfile {
    ModelRequestProfile {
        chat_parallel_tool_calls: true,
        ..ModelRequestProfile::default()
    }
}

// ---- effort 参数声明 ----

/// Zhipu 普通模型 effort：候选值按弱到强 none/enabled，映射 `thinking.type` 开关。
fn zhipu_plain_effort_parameter() -> ModelParameter {
    ModelParameter {
        name: "effort".to_string(),
        label: None,
        candidates: vec!["none".to_string(), "enabled".to_string()],
        wire: BTreeMap::from([
            (
                "enabled".to_string(),
                ParameterWire {
                    set: vec![
                        WireAssignment {
                            path: "thinking.type".to_string(),
                            value: Value::String("enabled".to_string()),
                        },
                        WireAssignment {
                            path: "thinking.clear_thinking".to_string(),
                            value: Value::Bool(false),
                        },
                    ],
                    remove: Vec::new(),
                },
            ),
            (
                "none".to_string(),
                ParameterWire {
                    set: vec![WireAssignment {
                        path: "thinking.type".to_string(),
                        value: Value::String("disabled".to_string()),
                    }],
                    remove: Vec::new(),
                },
            ),
        ]),
    }
}

/// GLM-5.2 effort：候选值按弱到强 none/high/max，联动 `reasoning_effort` 与 `thinking`。
fn zhipu_glm52_effort_parameter() -> ModelParameter {
    ModelParameter {
        name: "effort".to_string(),
        label: None,
        candidates: vec!["none".to_string(), "high".to_string(), "max".to_string()],
        wire: BTreeMap::from([
            ("high".to_string(), glm_reasoning_effort_wire("high")),
            ("max".to_string(), glm_reasoning_effort_wire("max")),
            (
                "none".to_string(),
                ParameterWire {
                    set: vec![WireAssignment {
                        path: "thinking.type".to_string(),
                        value: Value::String("disabled".to_string()),
                    }],
                    remove: vec!["reasoning_effort".to_string()],
                },
            ),
        ]),
    }
}

/// GLM-5.3 effort：候选值按弱到强 low/high/max，三档共用「始终思考」wire，仅切换 reasoning_effort。
fn zhipu_glm53_effort_parameter() -> ModelParameter {
    ModelParameter {
        name: "effort".to_string(),
        label: None,
        candidates: vec!["low".to_string(), "high".to_string(), "max".to_string()],
        wire: BTreeMap::from([
            ("high".to_string(), glm_reasoning_effort_wire("high")),
            ("low".to_string(), glm_reasoning_effort_wire("low")),
            ("max".to_string(), glm_reasoning_effort_wire("max")),
        ]),
    }
}

/// GLM-5.2/GLM-5.3 共用 wire：设 reasoning_effort + 开启 thinking。
fn glm_reasoning_effort_wire(effort: &str) -> ParameterWire {
    ParameterWire {
        set: vec![
            WireAssignment {
                path: "reasoning_effort".to_string(),
                value: Value::String(effort.to_string()),
            },
            WireAssignment {
                path: "thinking.type".to_string(),
                value: Value::String("enabled".to_string()),
            },
            WireAssignment {
                path: "thinking.clear_thinking".to_string(),
                value: Value::Bool(false),
            },
        ],
        remove: Vec::new(),
    }
}

// ---- 能力矩阵 ----

fn zhipu_text_capabilities() -> ModelCapabilities {
    zhipu_capabilities(vec![ModelInputCapability::text()])
}

fn zhipu_vision_capabilities() -> ModelCapabilities {
    zhipu_capabilities(vec![
        ModelInputCapability::text(),
        ModelInputCapability::media(
            ModelModality::Image,
            vec![ModelInputSource::Local, ModelInputSource::RemoteUrl],
        ),
    ])
}

fn zhipu_capabilities(input: Vec<ModelInputCapability>) -> ModelCapabilities {
    ModelCapabilities {
        streaming: true,
        temperature: false,
        reasoning: true,
        web_search: false,
        input,
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
