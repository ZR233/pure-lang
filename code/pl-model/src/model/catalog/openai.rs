//! OpenAI 内建模型目录：GPT 系列共享 Responses family，默认 WebSocket 连接。

use crate::model::capabilities::{
    ModelCapabilities, ModelInputCapability, ModelInputSource, ModelModality,
    PromptCacheModelCapabilities, ToolCapabilities,
};
use crate::model::family::{ModelFamily, ModelInstanceSpec};
use crate::model::info::{ModelInfo, ModelRequestProfile, ModelTransportProfile, TruncationMode};
use crate::model::parameter::ModelParameter;
use crate::model::pricing::{ModelPricing, TokenPriceTier};

const OPENAI_DEFAULT_MODEL_SLUGS: &[&str] = &[
    "gpt-6-astra",
    "gpt-5.5",
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
        openai_gpt56.instantiate(ModelInstanceSpec {
            slug: "gpt-6-astra",
            display_name: "GPT-6 Astra",
            description: "OpenAI flagship model for complex reasoning, coding and agent tasks.",
            context_window: 1_050_000,
            max_context_window: 1_050_000,
            max_output_tokens: Some(128_000),
            pricing: openai_pricing(10.0, 50.0, Some(12.5)),
        }),
        openai.instantiate(ModelInstanceSpec {
            slug: "gpt-5.5",
            display_name: "GPT-5.5",
            description: "Frontier model for complex coding, research, and real-world work.",
            context_window: 272_000,
            max_context_window: 272_000,
            max_output_tokens: None,
            pricing: openai_pricing(5.0, 30.0, None),
        }),
        openai_gpt56.instantiate(ModelInstanceSpec {
            slug: "gpt-5.6-sol",
            display_name: "GPT-5.6-Sol",
            description: "Latest frontier agentic coding model.",
            context_window: 272_000,
            max_context_window: 272_000,
            max_output_tokens: None,
            pricing: openai_pricing(4.0, 20.0, Some(5.0)),
        }),
        openai_gpt56.instantiate(ModelInstanceSpec {
            slug: "gpt-5.6-terra",
            display_name: "GPT-5.6-Terra",
            description: "Balanced agentic coding model for everyday work.",
            context_window: 272_000,
            max_context_window: 272_000,
            max_output_tokens: None,
            pricing: openai_pricing(2.0, 12.0, Some(2.5)),
        }),
        openai_gpt56.instantiate(ModelInstanceSpec {
            slug: "gpt-5.6-luna",
            display_name: "GPT-5.6-Luna",
            description: "Fast and affordable agentic coding model.",
            context_window: 272_000,
            max_context_window: 272_000,
            max_output_tokens: None,
            pricing: openai_pricing(0.2, 1.2, Some(0.25)),
        }),
    ]
}

fn openai_pricing(input: f64, output: f64, write: Option<f64>) -> ModelPricing {
    let mut short = TokenPriceTier::flat(input, output, Some(input * 0.1), write);
    short.input_until = Some(272_001);
    let mut long = TokenPriceTier::flat(
        input * 2.0,
        output * 1.5,
        Some(input * 0.2),
        write.map(|price| price * 2.0),
    );
    long.input_from = 272_001;
    ModelPricing::published(
        "USD",
        vec![short, long],
        "https://developers.openai.com/api/docs/pricing",
    )
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
        protocol: crate::model::ModelProtocolOptions::Responses(
            crate::model::ResponsesRequestOptions {
                programmatic_tool_calling: true,
                ..Default::default()
            },
        ),
        media: super::image_media_profiles(
            super::MediaWireFormat::ResponsesInputImage,
            super::MediaSendOrder::RemoteUrlFirst,
        ),
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
