//! 默认模型定义，按 provider 分组，用 `ModelFamily` 预设复用共享元数据。
//!
//! 参见 design/07-model.md 7.8 / 7.9 节。同 provider 的模型共享 capabilities、
//! truncation_policy、effort 参数声明（`ModelParameter`）和 base body，具体模型
//! 仅以差异字段从 family 派生。

use std::collections::BTreeMap;

use serde_json::Map;
use serde_json::Value;

use crate::capabilities::{
    ModelCapabilities, ModelModality, ReasoningInterleaved, ReasoningInterleavedField,
    ToolCapabilities,
};
use crate::model_family::{ModelFamily, ModelPricing};
use crate::model_info::{MaxTokensField, ModelInfo, ModelRequestProfile, TruncationMode};
use crate::parameter::{ModelParameter, ParameterWire, WireAssignment};

const DEEPSEEK_DEFAULT_MODEL_SLUGS: &[&str] = &["deepseek-v4-flash", "deepseek-v4-pro"];
const OPENAI_DEFAULT_MODEL_SLUGS: &[&str] = &[
    "gpt-5.5",
    "gpt-5.4",
    "gpt-5.4-mini",
    "gpt-5.6-sol",
    "gpt-5.6-terra",
    "gpt-5.6-luna",
];
const MIMO_DEFAULT_MODEL_SLUGS: &[&str] =
    &["mimo-v2.5-pro", "mimo-v2.5", "mimo-v2-pro", "mimo-v2-omni"];
const ZHIPU_GLM_DEFAULT_MODEL_SLUGS: &[&str] = &[
    "glm-5.2",
    "glm-5",
    "glm-5-turbo",
    "glm-4.7",
    "glm-4.7-flashx",
    "glm-4.7-flash",
];

pub fn deepseek_default_model_slugs() -> &'static [&'static str] {
    DEEPSEEK_DEFAULT_MODEL_SLUGS
}

pub fn openai_default_model_slugs() -> &'static [&'static str] {
    OPENAI_DEFAULT_MODEL_SLUGS
}

pub fn mimo_default_model_slugs() -> &'static [&'static str] {
    MIMO_DEFAULT_MODEL_SLUGS
}

pub fn zhipu_default_model_slugs() -> &'static [&'static str] {
    ZHIPU_GLM_DEFAULT_MODEL_SLUGS
}

pub fn default_models() -> Vec<ModelInfo> {
    let openai = openai_family();
    let openai_gpt56 = openai_gpt56_family("medium");
    let openai_gpt56_sol = openai_gpt56_family("low");
    let deepseek = deepseek_family();
    let mimo_text = mimo_family(false);
    let mimo_vision = mimo_family(true);
    let zhipu_text = zhipu_text_family();
    let zhipu_glm52 = zhipu_glm52_family();
    let zhipu_vision = zhipu_vision_family();

    vec![
        // DeepSeek
        deepseek.instantiate(
            "deepseek-v4-flash",
            "DeepSeek V4 Flash",
            "DeepSeek fast reasoning model with thinking mode.",
            1_000_000,
            1_000_000,
            Some(384_000),
            ModelPricing {
                currency: Some("CNY".to_string()),
                input_per_mtok: Some(1.0),
                output_per_mtok: Some(2.0),
                cache_read_per_mtok: Some(0.02),
            },
        ),
        deepseek.instantiate(
            "deepseek-v4-pro",
            "DeepSeek V4 Pro",
            "DeepSeek flagship reasoning model with thinking mode.",
            1_000_000,
            1_000_000,
            Some(384_000),
            ModelPricing {
                currency: Some("CNY".to_string()),
                input_per_mtok: Some(3.0),
                output_per_mtok: Some(6.0),
                cache_read_per_mtok: Some(0.025),
            },
        ),
        // OpenAI
        openai.instantiate(
            "gpt-5.5",
            "GPT-5.5",
            "Frontier model for complex coding, research, and real-world work.",
            272_000,
            272_000,
            None,
            ModelPricing::default(),
        ),
        openai.instantiate(
            "gpt-5.4",
            "gpt-5.4",
            "Strong model for everyday coding.",
            272_000,
            1_000_000,
            None,
            ModelPricing::default(),
        ),
        openai.instantiate(
            "gpt-5.4-mini",
            "GPT-5.4-Mini",
            "Small, fast, and cost-efficient model for simpler coding tasks.",
            272_000,
            272_000,
            None,
            ModelPricing::default(),
        ),
        openai_gpt56_sol.instantiate(
            "gpt-5.6-sol",
            "GPT-5.6-Sol",
            "Latest frontier agentic coding model.",
            272_000,
            272_000,
            None,
            ModelPricing::default(),
        ),
        openai_gpt56.instantiate(
            "gpt-5.6-terra",
            "GPT-5.6-Terra",
            "Balanced agentic coding model for everyday work.",
            272_000,
            272_000,
            None,
            ModelPricing::default(),
        ),
        openai_gpt56.instantiate(
            "gpt-5.6-luna",
            "GPT-5.6-Luna",
            "Fast and affordable agentic coding model.",
            272_000,
            272_000,
            None,
            ModelPricing::default(),
        ),
        // Xiaomi MiMo（OpenAI-compatible Chat）
        mimo_text.instantiate(
            "mimo-v2.5-pro",
            "MiMo V2.5 Pro",
            "Xiaomi MiMo flagship model for long-horizon agent work.",
            1_000_000,
            1_000_000,
            Some(131_072),
            ModelPricing::default(),
        ),
        mimo_vision.instantiate(
            "mimo-v2.5",
            "MiMo V2.5",
            "Xiaomi MiMo full-modal agent model with a one-million-token context window.",
            1_000_000,
            1_000_000,
            Some(32_768),
            ModelPricing::default(),
        ),
        mimo_text.instantiate(
            "mimo-v2-pro",
            "MiMo V2 Pro",
            "Xiaomi MiMo long-context reasoning model for complex agent tasks.",
            1_000_000,
            1_000_000,
            Some(131_072),
            ModelPricing::default(),
        ),
        mimo_vision.instantiate(
            "mimo-v2-omni",
            "MiMo V2 Omni",
            "Xiaomi MiMo multimodal model for text and visual agent tasks.",
            256_000,
            256_000,
            Some(32_768),
            ModelPricing::default(),
        ),
        // Zhipu GLM-5.2（effort 候选值 high/max/none，联动 reasoning_effort + thinking）
        zhipu_glm52.instantiate(
            "glm-5.2",
            "GLM-5.2",
            "Zhipu latest flagship model with stronger coding and long-horizon agent work.",
            1_000_000,
            1_000_000,
            Some(128_000),
            ModelPricing::default(),
        ),
        // Zhipu 文本模型（effort 候选值 enabled/none，映射 thinking 开关）
        zhipu_text.instantiate(
            "glm-5",
            "GLM-5",
            "Zhipu high-intelligence foundation model for coding and agentic planning.",
            200_000,
            200_000,
            Some(128_000),
            ModelPricing::default(),
        ),
        zhipu_text.instantiate(
            "glm-5-turbo",
            "GLM-5-Turbo",
            "Zhipu GLM-5 Turbo model optimized for long task continuity.",
            200_000,
            200_000,
            Some(128_000),
            ModelPricing::default(),
        ),
        zhipu_text.instantiate(
            "glm-4.7",
            "GLM-4.7",
            "Zhipu high-intelligence model for dialogue, reasoning, agents, and coding.",
            200_000,
            200_000,
            Some(128_000),
            ModelPricing::default(),
        ),
        zhipu_text.instantiate(
            "glm-4.7-flashx",
            "GLM-4.7-FlashX",
            "Zhipu lightweight high-speed model for general text tasks.",
            200_000,
            200_000,
            Some(128_000),
            ModelPricing::default(),
        ),
        zhipu_text.instantiate(
            "glm-4.6",
            "GLM-4.6",
            "Zhipu model with stronger coding, reasoning, and tool calling.",
            200_000,
            200_000,
            Some(128_000),
            ModelPricing::default(),
        ),
        zhipu_text.instantiate(
            "glm-4.5-air",
            "GLM-4.5-Air",
            "Zhipu cost-effective model for reasoning, coding, and agent tasks.",
            128_000,
            128_000,
            Some(96_000),
            ModelPricing::default(),
        ),
        zhipu_text.instantiate(
            "glm-4.5-airx",
            "GLM-4.5-AirX",
            "Zhipu fast cost-effective model for latency-sensitive tasks.",
            128_000,
            128_000,
            Some(96_000),
            ModelPricing::default(),
        ),
        zhipu_text.instantiate(
            "glm-4-long",
            "GLM-4-Long",
            "Zhipu long-context model for very large inputs and memory-heavy tasks.",
            1_000_000,
            1_000_000,
            Some(4_000),
            ModelPricing::default(),
        ),
        zhipu_text.instantiate(
            "glm-4-flashx-250414",
            "GLM-4-FlashX-250414",
            "Zhipu high-speed low-cost Flash model with higher concurrency.",
            128_000,
            128_000,
            Some(16_000),
            ModelPricing::default(),
        ),
        zhipu_text.instantiate(
            "glm-4.7-flash",
            "GLM-4.7-Flash",
            "Zhipu free GLM-4.7 base model.",
            200_000,
            200_000,
            Some(128_000),
            ModelPricing::default(),
        ),
        zhipu_text.instantiate(
            "glm-4.5-flash",
            "GLM-4.5-Flash",
            "Zhipu free GLM-4.5 model, marked by the official docs as being phased out.",
            128_000,
            128_000,
            Some(96_000),
            ModelPricing::default(),
        ),
        zhipu_text.instantiate(
            "glm-4-flash-250414",
            "GLM-4-Flash-250414",
            "Zhipu free GLM-4 Flash model for long-context multilingual tool use.",
            128_000,
            128_000,
            Some(16_000),
            ModelPricing::default(),
        ),
        // Zhipu 视觉模型
        zhipu_vision.instantiate(
            "glm-5v-turbo",
            "GLM-5V-Turbo",
            "Zhipu multimodal coding model for visual understanding and agent workflows.",
            200_000,
            200_000,
            Some(128_000),
            ModelPricing::default(),
        ),
        zhipu_vision.instantiate(
            "glm-4.6v",
            "GLM-4.6V",
            "Zhipu visual reasoning model with native tool calling.",
            128_000,
            128_000,
            Some(32_000),
            ModelPricing::default(),
        ),
        zhipu_vision.instantiate(
            "glm-4.1v-thinking-flashx",
            "GLM-4.1V-Thinking-FlashX",
            "Zhipu lightweight visual reasoning model for complex scene understanding.",
            64_000,
            64_000,
            Some(16_000),
            ModelPricing::default(),
        ),
        zhipu_vision.instantiate(
            "glm-4.6v-flash",
            "GLM-4.6V-Flash",
            "Zhipu free visual reasoning model with tool calling.",
            128_000,
            128_000,
            Some(32_000),
            ModelPricing::default(),
        ),
        zhipu_vision.instantiate(
            "glm-4.1v-thinking-flash",
            "GLM-4.1V-Thinking-Flash",
            "Zhipu free visual reasoning model for multi-step analysis.",
            64_000,
            64_000,
            Some(16_000),
            ModelPricing::default(),
        ),
        zhipu_vision.instantiate(
            "glm-4v-flash",
            "GLM-4V-Flash",
            "Zhipu free image understanding model with multilingual support.",
            16_000,
            16_000,
            Some(1_000),
            ModelPricing::default(),
        ),
    ]
}

fn openai_family() -> ModelFamily {
    ModelFamily {
        id: "openai-reasoning",
        capabilities: openai_capabilities(),
        truncation_mode: TruncationMode::Tokens,
        truncation_limit: 10_000,
        parameters: vec![openai_effort_parameter(&["medium", "low", "high", "xhigh"])],
        request_profile: ModelRequestProfile::default(),
        base_instructions: String::new(),
    }
}

fn openai_gpt56_family(default_effort: &str) -> ModelFamily {
    let mut candidates = vec![default_effort];
    for effort in ["low", "medium", "high", "xhigh", "max"] {
        if effort != default_effort {
            candidates.push(effort);
        }
    }

    ModelFamily {
        id: "openai-gpt56-reasoning",
        capabilities: openai_capabilities(),
        truncation_mode: TruncationMode::Tokens,
        truncation_limit: 10_000,
        parameters: vec![openai_effort_parameter(&candidates)],
        request_profile: ModelRequestProfile::default(),
        base_instructions: String::new(),
    }
}

fn deepseek_family() -> ModelFamily {
    ModelFamily {
        id: "deepseek-reasoning",
        capabilities: deepseek_capabilities(),
        truncation_mode: TruncationMode::Tokens,
        truncation_limit: 10_000,
        parameters: vec![deepseek_effort_parameter()],
        request_profile: deepseek_request_profile(),
        base_instructions: String::new(),
    }
}

fn zhipu_text_family() -> ModelFamily {
    ModelFamily {
        id: "zhipu-text",
        capabilities: zhipu_capabilities(false),
        truncation_mode: TruncationMode::Tokens,
        truncation_limit: 10_000,
        parameters: vec![zhipu_plain_effort_parameter()],
        request_profile: ModelRequestProfile::default(),
        base_instructions: String::new(),
    }
}

fn mimo_family(vision: bool) -> ModelFamily {
    ModelFamily {
        id: if vision { "mimo-vision" } else { "mimo-text" },
        capabilities: mimo_capabilities(vision),
        truncation_mode: TruncationMode::Tokens,
        truncation_limit: 10_000,
        parameters: vec![mimo_effort_parameter()],
        request_profile: ModelRequestProfile {
            max_tokens_field: MaxTokensField::MaxCompletionTokens,
            ..ModelRequestProfile::default()
        },
        base_instructions: String::new(),
    }
}

fn zhipu_glm52_family() -> ModelFamily {
    ModelFamily {
        id: "zhipu-glm52",
        capabilities: zhipu_capabilities(false),
        truncation_mode: TruncationMode::Tokens,
        truncation_limit: 10_000,
        parameters: vec![zhipu_glm52_effort_parameter()],
        request_profile: ModelRequestProfile::default(),
        base_instructions: String::new(),
    }
}

fn zhipu_vision_family() -> ModelFamily {
    ModelFamily {
        id: "zhipu-vision",
        capabilities: zhipu_capabilities(true),
        truncation_mode: TruncationMode::Tokens,
        truncation_limit: 10_000,
        parameters: vec![zhipu_plain_effort_parameter()],
        request_profile: ModelRequestProfile::default(),
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
        ..ModelRequestProfile::default()
    }
}

// ---- effort 参数声明 ----

/// OpenAI effort：候选值按模型声明顺序透传到 Responses 的 `reasoning.effort`。
fn openai_effort_parameter(candidates: &[&str]) -> ModelParameter {
    let candidates = candidates
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    ModelParameter {
        name: "effort".to_string(),
        label: None,
        wire: candidates
            .iter()
            .map(|value| (value.clone(), wire_set_one("reasoning.effort", value)))
            .collect(),
        candidates,
    }
}

/// DeepSeek effort：候选值 high/max，透传到 `reasoning_effort`。
fn deepseek_effort_parameter() -> ModelParameter {
    ModelParameter {
        name: "effort".to_string(),
        label: None,
        candidates: vec!["high".to_string(), "max".to_string()],
        wire: ["high", "max"]
            .into_iter()
            .map(|value| (value.to_string(), wire_set_one("reasoning_effort", value)))
            .collect(),
    }
}

/// MiMo thinking 开关；官方 Chat API 只声明 `thinking.type`。
fn mimo_effort_parameter() -> ModelParameter {
    ModelParameter {
        name: "effort".to_string(),
        label: Some("Thinking".to_string()),
        candidates: vec!["enabled".to_string(), "disabled".to_string()],
        wire: ["enabled", "disabled"]
            .into_iter()
            .map(|value| (value.to_string(), wire_set_one("thinking.type", value)))
            .collect(),
    }
}

/// Zhipu 普通模型 effort：候选值 enabled/none，映射 `thinking.type` 开关。
fn zhipu_plain_effort_parameter() -> ModelParameter {
    ModelParameter {
        name: "effort".to_string(),
        label: None,
        candidates: vec!["enabled".to_string(), "none".to_string()],
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

/// GLM-5.2 effort：候选值 high/max/none，联动 `reasoning_effort` 与 `thinking`。
fn zhipu_glm52_effort_parameter() -> ModelParameter {
    ModelParameter {
        name: "effort".to_string(),
        label: None,
        candidates: vec!["high".to_string(), "max".to_string(), "none".to_string()],
        wire: BTreeMap::from([
            ("high".to_string(), glm52_enabled_wire("high")),
            ("max".to_string(), glm52_enabled_wire("max")),
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

/// GLM-5.2 high/max 共用 wire：设 reasoning_effort + 开启 thinking。
fn glm52_enabled_wire(effort: &str) -> ParameterWire {
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

/// 构造「把单个字段设为字符串值」的 wire。
fn wire_set_one(path: &str, value: &str) -> ParameterWire {
    ParameterWire {
        set: vec![WireAssignment {
            path: path.to_string(),
            value: Value::String(value.to_string()),
        }],
        remove: Vec::new(),
    }
}

// ---- 能力矩阵 ----

fn openai_capabilities() -> ModelCapabilities {
    ModelCapabilities {
        streaming: true,
        temperature: false,
        reasoning: true,
        web_search: true,
        input: vec![ModelModality::Text, ModelModality::Image],
        output: vec![ModelModality::Text],
        tools: ToolCapabilities {
            function_calling: true,
            parallel_tool_calls: true,
            custom_tools: true,
            freeform_tools: true,
        },
        interleaved: None,
    }
}

fn deepseek_capabilities() -> ModelCapabilities {
    ModelCapabilities {
        streaming: true,
        temperature: false,
        reasoning: true,
        web_search: false,
        input: vec![ModelModality::Text],
        output: vec![ModelModality::Text],
        tools: ToolCapabilities {
            function_calling: true,
            parallel_tool_calls: true,
            custom_tools: false,
            freeform_tools: false,
        },
        interleaved: Some(ReasoningInterleaved {
            field: ReasoningInterleavedField::ReasoningContent,
        }),
    }
}

fn mimo_capabilities(vision: bool) -> ModelCapabilities {
    let mut input = vec![ModelModality::Text];
    if vision {
        input.push(ModelModality::Image);
    }
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
        },
        interleaved: Some(ReasoningInterleaved {
            field: ReasoningInterleavedField::ReasoningContent,
        }),
    }
}

fn zhipu_capabilities(vision: bool) -> ModelCapabilities {
    let mut input = vec![ModelModality::Text];
    if vision {
        input.push(ModelModality::Image);
    }
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
        },
        interleaved: Some(ReasoningInterleaved {
            field: ReasoningInterleavedField::ReasoningContent,
        }),
    }
}

#[cfg(test)]
mod tests;
