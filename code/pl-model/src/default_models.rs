use crate::capabilities::{
    ModelCapabilities, ModelModality, ReasoningInterleaved, ReasoningInterleavedField,
    ToolCapabilities,
};
use crate::model_info::{ModelInfo, ModelRequestProfile, TruncationMode, TruncationPolicy};

const DEEPSEEK_DEFAULT_MODEL_SLUGS: &[&str] = &["deepseek-v4-flash", "deepseek-v4-pro"];
const OPENAI_DEFAULT_MODEL_SLUGS: &[&str] = &[
    "gpt-5.5",
    "gpt-5.4",
    "gpt-5.4-mini",
    "gpt-5.3-codex",
    "gpt-5.2",
];
const ZHIPU_GLM_DEFAULT_MODEL_SLUGS: &[&str] = &[
    "glm-5.1",
    "glm-5",
    "glm-5-turbo",
    "glm-4.7",
    "glm-4.7-flashx",
    "glm-4.7-flash",
];
const OPENAI_REASONING_EFFORTS: &[&str] = &["medium", "low", "high", "xhigh"];
const DEEPSEEK_REASONING_EFFORTS: &[&str] = &["high", "max"];
const ZHIPU_REASONING_EFFORTS: &[&str] = &["enabled", "none"];

pub fn deepseek_default_model_slugs() -> &'static [&'static str] {
    DEEPSEEK_DEFAULT_MODEL_SLUGS
}

pub fn openai_default_model_slugs() -> &'static [&'static str] {
    OPENAI_DEFAULT_MODEL_SLUGS
}

pub fn zhipu_default_model_slugs() -> &'static [&'static str] {
    ZHIPU_GLM_DEFAULT_MODEL_SLUGS
}

pub fn default_models() -> Vec<ModelInfo> {
    vec![
        deepseek_model(
            "deepseek-v4-flash",
            "DeepSeek V4 Flash",
            "DeepSeek fast reasoning model with thinking mode.",
            DeepSeekPrice {
                cache_read_per_mtok: 0.02,
                input_per_mtok: 1.0,
                output_per_mtok: 2.0,
            },
        ),
        deepseek_model(
            "deepseek-v4-pro",
            "DeepSeek V4 Pro",
            "DeepSeek flagship reasoning model with thinking mode.",
            DeepSeekPrice {
                cache_read_per_mtok: 0.025,
                input_per_mtok: 3.0,
                output_per_mtok: 6.0,
            },
        ),
        openai_model(
            "gpt-5.5",
            "GPT-5.5",
            "Frontier model for complex coding, research, and real-world work.",
            272_000,
            272_000,
            TruncationMode::Tokens,
        ),
        openai_model(
            "gpt-5.4",
            "gpt-5.4",
            "Strong model for everyday coding.",
            272_000,
            1_000_000,
            TruncationMode::Tokens,
        ),
        openai_model(
            "gpt-5.4-mini",
            "GPT-5.4-Mini",
            "Small, fast, and cost-efficient model for simpler coding tasks.",
            272_000,
            272_000,
            TruncationMode::Tokens,
        ),
        openai_model(
            "gpt-5.3-codex",
            "gpt-5.3-codex",
            "Coding-optimized model.",
            272_000,
            272_000,
            TruncationMode::Tokens,
        ),
        openai_model(
            "gpt-5.2",
            "gpt-5.2",
            "Optimized for professional work and long-running agents.",
            272_000,
            272_000,
            TruncationMode::Bytes,
        ),
        zhipu_text_model(
            "glm-5.1",
            "GLM-5.1",
            "Zhipu latest flagship model with stronger coding and long-horizon agent work.",
            200_000,
            128_000,
        ),
        zhipu_text_model(
            "glm-5",
            "GLM-5",
            "Zhipu high-intelligence foundation model for coding and agentic planning.",
            200_000,
            128_000,
        ),
        zhipu_text_model(
            "glm-5-turbo",
            "GLM-5-Turbo",
            "Zhipu GLM-5 Turbo model optimized for long task continuity.",
            200_000,
            128_000,
        ),
        zhipu_text_model(
            "glm-4.7",
            "GLM-4.7",
            "Zhipu high-intelligence model for dialogue, reasoning, agents, and coding.",
            200_000,
            128_000,
        ),
        zhipu_text_model(
            "glm-4.7-flashx",
            "GLM-4.7-FlashX",
            "Zhipu lightweight high-speed model for general text tasks.",
            200_000,
            128_000,
        ),
        zhipu_text_model(
            "glm-4.6",
            "GLM-4.6",
            "Zhipu model with stronger coding, reasoning, and tool calling.",
            200_000,
            128_000,
        ),
        zhipu_text_model(
            "glm-4.5-air",
            "GLM-4.5-Air",
            "Zhipu cost-effective model for reasoning, coding, and agent tasks.",
            128_000,
            96_000,
        ),
        zhipu_text_model(
            "glm-4.5-airx",
            "GLM-4.5-AirX",
            "Zhipu fast cost-effective model for latency-sensitive tasks.",
            128_000,
            96_000,
        ),
        zhipu_text_model(
            "glm-4-long",
            "GLM-4-Long",
            "Zhipu long-context model for very large inputs and memory-heavy tasks.",
            1_000_000,
            4_000,
        ),
        zhipu_text_model(
            "glm-4-flashx-250414",
            "GLM-4-FlashX-250414",
            "Zhipu high-speed low-cost Flash model with higher concurrency.",
            128_000,
            16_000,
        ),
        zhipu_text_model(
            "glm-4.7-flash",
            "GLM-4.7-Flash",
            "Zhipu free GLM-4.7 base model.",
            200_000,
            128_000,
        ),
        zhipu_text_model(
            "glm-4.5-flash",
            "GLM-4.5-Flash",
            "Zhipu free GLM-4.5 model, marked by the official docs as being phased out.",
            128_000,
            96_000,
        ),
        zhipu_text_model(
            "glm-4-flash-250414",
            "GLM-4-Flash-250414",
            "Zhipu free GLM-4 Flash model for long-context multilingual tool use.",
            128_000,
            16_000,
        ),
        zhipu_vision_model(
            "glm-5v-turbo",
            "GLM-5V-Turbo",
            "Zhipu multimodal coding model for visual understanding and agent workflows.",
            200_000,
            128_000,
        ),
        zhipu_vision_model(
            "glm-4.6v",
            "GLM-4.6V",
            "Zhipu visual reasoning model with native tool calling.",
            128_000,
            32_000,
        ),
        zhipu_vision_model(
            "glm-4.1v-thinking-flashx",
            "GLM-4.1V-Thinking-FlashX",
            "Zhipu lightweight visual reasoning model for complex scene understanding.",
            64_000,
            16_000,
        ),
        zhipu_vision_model(
            "glm-4.6v-flash",
            "GLM-4.6V-Flash",
            "Zhipu free visual reasoning model with tool calling.",
            128_000,
            32_000,
        ),
        zhipu_vision_model(
            "glm-4.1v-thinking-flash",
            "GLM-4.1V-Thinking-Flash",
            "Zhipu free visual reasoning model for multi-step analysis.",
            64_000,
            16_000,
        ),
        zhipu_vision_model(
            "glm-4v-flash",
            "GLM-4V-Flash",
            "Zhipu free image understanding model with multilingual support.",
            16_000,
            1_000,
        ),
    ]
}

fn openai_model(
    slug: &str,
    display_name: &str,
    description: &str,
    context_window: u64,
    max_context_window: u64,
    truncation_mode: TruncationMode,
) -> ModelInfo {
    ModelInfo {
        slug: slug.to_string(),
        display_name: display_name.to_string(),
        description: Some(description.to_string()),
        context_window: Some(context_window),
        max_context_window: Some(max_context_window),
        auto_compact_token_limit: None,
        default_temperature: None,
        max_output_tokens: None,
        currency: None,
        input_price_per_mtok: None,
        output_price_per_mtok: None,
        cache_read_price_per_mtok: None,
        reasoning_efforts: OPENAI_REASONING_EFFORTS
            .iter()
            .copied()
            .map(String::from)
            .collect(),
        capabilities: openai_capabilities(),
        request_profile: ModelRequestProfile::default(),
        truncation_policy: TruncationPolicy {
            mode: truncation_mode,
            limit: 10_000,
        },
        base_instructions: String::new(),
        used_fallback: false,
    }
}

struct DeepSeekPrice {
    cache_read_per_mtok: f64,
    input_per_mtok: f64,
    output_per_mtok: f64,
}

fn deepseek_model(
    slug: &str,
    display_name: &str,
    description: &str,
    price: DeepSeekPrice,
) -> ModelInfo {
    ModelInfo {
        slug: slug.to_string(),
        display_name: display_name.to_string(),
        description: Some(description.to_string()),
        context_window: Some(1_000_000),
        max_context_window: Some(1_000_000),
        auto_compact_token_limit: None,
        default_temperature: None,
        max_output_tokens: Some(384_000),
        currency: Some("CNY".to_string()),
        input_price_per_mtok: Some(price.input_per_mtok),
        output_price_per_mtok: Some(price.output_per_mtok),
        cache_read_price_per_mtok: Some(price.cache_read_per_mtok),
        reasoning_efforts: DEEPSEEK_REASONING_EFFORTS
            .iter()
            .copied()
            .map(String::from)
            .collect(),
        capabilities: deepseek_capabilities(),
        request_profile: ModelRequestProfile::default(),
        truncation_policy: TruncationPolicy {
            mode: TruncationMode::Tokens,
            limit: 10_000,
        },
        base_instructions: String::new(),
        used_fallback: false,
    }
}

fn zhipu_text_model(
    slug: &str,
    display_name: &str,
    description: &str,
    context_window: u64,
    max_output_tokens: u64,
) -> ModelInfo {
    zhipu_model(
        slug,
        display_name,
        description,
        context_window,
        max_output_tokens,
        zhipu_capabilities(false),
    )
}

fn zhipu_vision_model(
    slug: &str,
    display_name: &str,
    description: &str,
    context_window: u64,
    max_output_tokens: u64,
) -> ModelInfo {
    zhipu_model(
        slug,
        display_name,
        description,
        context_window,
        max_output_tokens,
        zhipu_capabilities(true),
    )
}

fn zhipu_model(
    slug: &str,
    display_name: &str,
    description: &str,
    context_window: u64,
    max_output_tokens: u64,
    capabilities: ModelCapabilities,
) -> ModelInfo {
    ModelInfo {
        slug: slug.to_string(),
        display_name: display_name.to_string(),
        description: Some(description.to_string()),
        context_window: Some(context_window),
        max_context_window: Some(context_window),
        auto_compact_token_limit: None,
        default_temperature: None,
        max_output_tokens: Some(max_output_tokens),
        currency: None,
        input_price_per_mtok: None,
        output_price_per_mtok: None,
        cache_read_price_per_mtok: None,
        reasoning_efforts: ZHIPU_REASONING_EFFORTS
            .iter()
            .copied()
            .map(String::from)
            .collect(),
        capabilities,
        request_profile: ModelRequestProfile::default(),
        truncation_policy: TruncationPolicy {
            mode: TruncationMode::Tokens,
            limit: 10_000,
        },
        base_instructions: String::new(),
        used_fallback: false,
    }
}

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
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn openai_default_models_match_codex_metadata() {
        let models = default_models();

        let openai_models = openai_default_model_slugs()
            .iter()
            .map(|slug| models.iter().find(|model| model.slug == *slug).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            openai_models
                .iter()
                .map(|model| model.slug.as_str())
                .collect::<Vec<_>>(),
            vec![
                "gpt-5.5",
                "gpt-5.4",
                "gpt-5.4-mini",
                "gpt-5.3-codex",
                "gpt-5.2"
            ]
        );

        let gpt_55 = openai_models[0];
        assert_eq!(gpt_55.display_name, "GPT-5.5");
        assert_eq!(gpt_55.context_window, Some(272_000));
        assert_eq!(gpt_55.max_context_window, Some(272_000));
        assert_eq!(gpt_55.max_output_tokens, None);
        assert_eq!(
            gpt_55.reasoning_efforts,
            vec!["medium", "low", "high", "xhigh"]
        );
        assert_eq!(gpt_55.truncation_policy.mode, TruncationMode::Tokens);
        assert!(gpt_55.capabilities.web_search);
        assert!(gpt_55.capabilities.tools.freeform_tools);

        let gpt_54 = openai_models[1];
        assert_eq!(gpt_54.display_name, "gpt-5.4");
        assert_eq!(gpt_54.context_window, Some(272_000));
        assert_eq!(gpt_54.max_context_window, Some(1_000_000));

        let gpt_52 = openai_models[4];
        assert_eq!(gpt_52.truncation_policy.mode, TruncationMode::Bytes);
        assert!(!models.iter().any(|model| model.slug == "gpt-5.4-nano"));
    }

    #[test]
    fn provider_default_model_slugs_are_backed_by_default_models() {
        let models = default_models();

        for slug in deepseek_default_model_slugs()
            .iter()
            .chain(openai_default_model_slugs())
            .chain(zhipu_default_model_slugs())
        {
            assert!(models.iter().any(|model| model.slug == *slug));
        }
    }

    #[test]
    fn default_models_include_deepseek_v4_models() {
        let models = default_models();

        for slug in deepseek_default_model_slugs() {
            let model = models.iter().find(|model| model.slug == *slug).unwrap();

            assert_eq!(model.context_window, Some(1_000_000));
            assert_eq!(model.max_output_tokens, Some(384_000));
            assert_eq!(model.currency.as_deref(), Some("CNY"));
            assert!(model.reasoning_efforts.iter().any(|effort| effort == "max"));
        }
    }

    #[test]
    fn deepseek_default_models_use_china_pricing() {
        let models = default_models();
        let flash = models
            .iter()
            .find(|model| model.slug == "deepseek-v4-flash")
            .unwrap();
        let pro = models
            .iter()
            .find(|model| model.slug == "deepseek-v4-pro")
            .unwrap();

        assert_eq!(flash.cache_read_price_per_mtok, Some(0.02));
        assert_eq!(flash.input_price_per_mtok, Some(1.0));
        assert_eq!(flash.output_price_per_mtok, Some(2.0));
        assert_eq!(pro.cache_read_price_per_mtok, Some(0.025));
        assert_eq!(pro.input_price_per_mtok, Some(3.0));
        assert_eq!(pro.output_price_per_mtok, Some(6.0));
    }

    #[test]
    fn default_models_include_zhipu_glm_models_from_official_overview() {
        let models = default_models();

        for slug in [
            "glm-5.1",
            "glm-5",
            "glm-5-turbo",
            "glm-4.7",
            "glm-4.7-flashx",
            "glm-4.6",
            "glm-4.5-air",
            "glm-4.5-airx",
            "glm-4-long",
            "glm-4-flashx-250414",
            "glm-4.7-flash",
            "glm-4.5-flash",
            "glm-4-flash-250414",
            "glm-5v-turbo",
            "glm-4.6v",
            "glm-4.1v-thinking-flashx",
            "glm-4.6v-flash",
            "glm-4.1v-thinking-flash",
            "glm-4v-flash",
        ] {
            let model = models.iter().find(|model| model.slug == *slug).unwrap();

            assert!(model.context_window.is_some());
            assert!(model.max_output_tokens.is_some());
            assert_eq!(model.currency, None);
            assert!(
                model
                    .reasoning_efforts
                    .iter()
                    .any(|effort| effort == "enabled")
            );
        }

        let glm_51 = models.iter().find(|model| model.slug == "glm-5.1").unwrap();
        assert_eq!(glm_51.display_name, "GLM-5.1");
        assert_eq!(glm_51.context_window, Some(200_000));
        assert_eq!(glm_51.max_output_tokens, Some(128_000));

        let glm_5v = models
            .iter()
            .find(|model| model.slug == "glm-5v-turbo")
            .unwrap();
        assert_eq!(
            glm_5v.capabilities.input,
            vec![ModelModality::Text, ModelModality::Image]
        );
        assert!(
            glm_5v
                .capabilities
                .supports_input_modality(ModelModality::Image)
        );
    }

    #[test]
    fn zhipu_default_model_list_excludes_phasing_out_glm_45_flash() {
        assert_eq!(
            zhipu_default_model_slugs(),
            [
                "glm-5.1",
                "glm-5",
                "glm-5-turbo",
                "glm-4.7",
                "glm-4.7-flashx",
                "glm-4.7-flash"
            ]
        );
        assert!(!zhipu_default_model_slugs().contains(&"glm-4.5-flash"));
        assert!(!zhipu_default_model_slugs().contains(&"glm-5v-turbo"));
        assert!(
            default_models()
                .iter()
                .any(|model| model.slug == "glm-4.5-flash")
        );
    }
}
