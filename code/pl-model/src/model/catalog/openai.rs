//! OpenAI 内建模型目录：GPT 系列共享 Responses family，默认 WebSocket 连接。

use crate::model::capabilities::{
    ModelCapabilities, ModelInputCapability, ModelInputSource, ModelModality,
    PromptCacheModelCapabilities, ToolCapabilities,
};
use crate::model::family::{ModelFamily, ModelInstanceSpec, ModelPricing};
use crate::model::info::{ModelInfo, ModelRequestProfile, ModelTransportProfile, TruncationMode};
use crate::model::parameter::ModelParameter;

const OPENAI_DEFAULT_MODEL_SLUGS: &[&str] = &[
    "gpt-5.5",
    "gpt-5.4",
    "gpt-5.4-mini",
    "gpt-5.6-sol",
    "gpt-5.6-terra",
    "gpt-5.6-luna",
];

pub fn openai_default_model_slugs() -> &'static [&'static str] {
    OPENAI_DEFAULT_MODEL_SLUGS
}

pub(super) fn models() -> Vec<ModelInfo> {
    let openai = openai_family();
    let openai_gpt56 = openai_gpt56_family();
    vec![
        openai.instantiate(ModelInstanceSpec {
            slug: "gpt-5.5",
            display_name: "GPT-5.5",
            description: "Frontier model for complex coding, research, and real-world work.",
            context_window: 272_000,
            max_context_window: 272_000,
            max_output_tokens: None,
            pricing: ModelPricing::default(),
        }),
        openai.instantiate(ModelInstanceSpec {
            slug: "gpt-5.4",
            display_name: "gpt-5.4",
            description: "Strong model for everyday coding.",
            context_window: 272_000,
            max_context_window: 1_000_000,
            max_output_tokens: None,
            pricing: ModelPricing::default(),
        }),
        openai.instantiate(ModelInstanceSpec {
            slug: "gpt-5.4-mini",
            display_name: "GPT-5.4-Mini",
            description: "Small, fast, and cost-efficient model for simpler coding tasks.",
            context_window: 272_000,
            max_context_window: 272_000,
            max_output_tokens: None,
            pricing: ModelPricing::default(),
        }),
        openai_gpt56.instantiate(ModelInstanceSpec {
            slug: "gpt-5.6-sol",
            display_name: "GPT-5.6-Sol",
            description: "Latest frontier agentic coding model.",
            context_window: 272_000,
            max_context_window: 272_000,
            max_output_tokens: None,
            pricing: ModelPricing::default(),
        }),
        openai_gpt56.instantiate(ModelInstanceSpec {
            slug: "gpt-5.6-terra",
            display_name: "GPT-5.6-Terra",
            description: "Balanced agentic coding model for everyday work.",
            context_window: 272_000,
            max_context_window: 272_000,
            max_output_tokens: None,
            pricing: ModelPricing::default(),
        }),
        openai_gpt56.instantiate(ModelInstanceSpec {
            slug: "gpt-5.6-luna",
            display_name: "GPT-5.6-Luna",
            description: "Fast and affordable agentic coding model.",
            context_window: 272_000,
            max_context_window: 272_000,
            max_output_tokens: None,
            pricing: ModelPricing::default(),
        }),
    ]
}

fn openai_family() -> ModelFamily {
    ModelFamily {
        id: "openai-reasoning",
        capabilities: openai_capabilities(),
        truncation_mode: TruncationMode::Tokens,
        truncation_limit: 10_000,
        parameters: vec![openai_effort_parameter(&["low", "medium", "high", "xhigh"])],
        transport: ModelTransportProfile::responses_websocket(),
        request_profile: openai_responses_request_profile(),
        base_instructions: String::new(),
    }
}

fn openai_gpt56_family() -> ModelFamily {
    let mut capabilities = openai_capabilities();
    capabilities.prompt_cache = PromptCacheModelCapabilities {
        cache_write_tokens: true,
    };
    ModelFamily {
        id: "openai-gpt56-reasoning",
        capabilities,
        truncation_mode: TruncationMode::Tokens,
        truncation_limit: 10_000,
        parameters: vec![openai_effort_parameter(&[
            "low", "medium", "high", "xhigh", "max",
        ])],
        transport: ModelTransportProfile::responses_websocket(),
        request_profile: openai_responses_request_profile(),
        base_instructions: String::new(),
    }
}

fn openai_responses_request_profile() -> ModelRequestProfile {
    ModelRequestProfile {
        responses_programmatic_tool_calling: true,
        media: super::image_media_profiles(super::MediaWireFormat::ResponsesInputImage, true),
        ..ModelRequestProfile::default()
    }
}

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
            .map(|value| {
                (
                    value.clone(),
                    super::wire_set_one("reasoning.effort", value),
                )
            })
            .collect(),
        candidates,
    }
}

fn openai_capabilities() -> ModelCapabilities {
    ModelCapabilities {
        streaming: true,
        temperature: false,
        reasoning: true,
        web_search: true,
        input: vec![
            ModelInputCapability::text(),
            ModelInputCapability::media(
                ModelModality::Image,
                vec![ModelInputSource::Local, ModelInputSource::RemoteUrl],
            ),
        ],
        output: vec![ModelModality::Text],
        tools: ToolCapabilities {
            function_calling: true,
            parallel_tool_calls: true,
            custom_tools: true,
            freeform_tools: true,
            programmatic_tool_calling: true,
        },
        interleaved: None,
        prompt_cache: PromptCacheModelCapabilities::default(),
    }
}
