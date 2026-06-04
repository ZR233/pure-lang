use crate::capabilities::ModelCapabilities;
use crate::model_info::{InputModality, ModelInfo, TruncationMode, TruncationPolicy};

const DEEPSEEK_DEFAULT_MODEL_SLUGS: &[&str] = &["deepseek-v4-flash", "deepseek-v4-pro"];
const OPENAI_DEFAULT_MODEL_SLUGS: &[&str] = &["gpt-5.5", "gpt-5.4", "gpt-5.4-mini", "gpt-5.4-nano"];
const ZHIPU_GLM_DEFAULT_MODEL_SLUGS: &[&str] = &[
    "glm-5.1",
    "glm-5",
    "glm-5-turbo",
    "glm-4.7",
    "glm-4.7-flashx",
    "glm-4.7-flash",
];
const OPENAI_REASONING_EFFORTS: &[&str] = &["none", "low", "medium", "high", "xhigh"];
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
            "Frontier model for complex reasoning, coding, and professional work.",
            1_050_000,
        ),
        openai_model(
            "gpt-5.4",
            "GPT-5.4",
            "Affordable frontier model for coding and professional work.",
            1_050_000,
        ),
        openai_model(
            "gpt-5.4-mini",
            "GPT-5.4 Mini",
            "Efficient GPT-5.4-class model for coding, computer use, and subagents.",
            400_000,
        ),
        openai_model(
            "gpt-5.4-nano",
            "GPT-5.4 Nano",
            "Lowest-cost GPT-5.4-class model for simple high-volume tasks.",
            400_000,
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
) -> ModelInfo {
    ModelInfo {
        slug: slug.to_string(),
        display_name: display_name.to_string(),
        description: Some(description.to_string()),
        context_window: Some(context_window),
        max_context_window: Some(context_window),
        auto_compact_token_limit: None,
        default_temperature: None,
        max_output_tokens: Some(128_000),
        currency: None,
        input_price_per_mtok: None,
        output_price_per_mtok: None,
        cache_read_price_per_mtok: None,
        reasoning_efforts: OPENAI_REASONING_EFFORTS
            .iter()
            .copied()
            .map(String::from)
            .collect(),
        capabilities: ModelCapabilities::STREAMING
            | ModelCapabilities::FUNCTION_CALLING
            | ModelCapabilities::VISION
            | ModelCapabilities::PARALLEL_TOOL_CALLS
            | ModelCapabilities::REASONING
            | ModelCapabilities::WEB_SEARCH
            | ModelCapabilities::CUSTOM_TOOLS
            | ModelCapabilities::FREEFORM_TOOLS,
        input_modalities: vec![InputModality::Text, InputModality::Image],
        truncation_policy: TruncationPolicy {
            mode: TruncationMode::Tokens,
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
        capabilities: ModelCapabilities::STREAMING
            | ModelCapabilities::FUNCTION_CALLING
            | ModelCapabilities::PARALLEL_TOOL_CALLS
            | ModelCapabilities::REASONING,
        input_modalities: vec![InputModality::Text],
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
        vec![InputModality::Text],
        ModelCapabilities::STREAMING
            | ModelCapabilities::FUNCTION_CALLING
            | ModelCapabilities::PARALLEL_TOOL_CALLS
            | ModelCapabilities::REASONING,
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
        vec![InputModality::Text, InputModality::Image],
        ModelCapabilities::STREAMING
            | ModelCapabilities::FUNCTION_CALLING
            | ModelCapabilities::VISION
            | ModelCapabilities::PARALLEL_TOOL_CALLS
            | ModelCapabilities::REASONING,
    )
}

fn zhipu_model(
    slug: &str,
    display_name: &str,
    description: &str,
    context_window: u64,
    max_output_tokens: u64,
    input_modalities: Vec<InputModality>,
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
        input_modalities,
        truncation_policy: TruncationPolicy {
            mode: TruncationMode::Tokens,
            limit: 10_000,
        },
        base_instructions: String::new(),
        used_fallback: false,
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn default_models_include_gpt_55_reasoning_efforts() {
        let models = default_models();

        assert!(!models.is_empty());
        let model = models.iter().find(|model| model.slug == "gpt-5.5").unwrap();

        assert!(
            model
                .reasoning_efforts
                .iter()
                .any(|effort| effort == "xhigh")
        );
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
            glm_5v.input_modalities,
            vec![InputModality::Text, InputModality::Image]
        );
        assert!(glm_5v.capabilities.contains(ModelCapabilities::VISION));
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
