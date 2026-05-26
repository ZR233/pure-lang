use crate::capabilities::ModelCapabilities;
use crate::model_info::{InputModality, ModelInfo, TruncationMode, TruncationPolicy};

const DEEPSEEK_DEFAULT_MODEL_SLUGS: &[&str] = &["deepseek-v4-flash", "deepseek-v4-pro"];
const OPENAI_DEFAULT_MODEL_SLUGS: &[&str] = &["gpt-5.5", "gpt-5.4", "gpt-5.4-mini", "gpt-5.4-nano"];
const DEFAULT_MODEL_SLUGS: &[&str] = &[
    "deepseek-v4-flash",
    "deepseek-v4-pro",
    "gpt-5.5",
    "gpt-5.4",
    "gpt-5.4-mini",
    "gpt-5.4-nano",
];
const OPENAI_REASONING_EFFORTS: &[&str] = &["none", "low", "medium", "high", "xhigh"];
const DEEPSEEK_REASONING_EFFORTS: &[&str] = &["high", "max"];

pub fn default_model_slugs() -> &'static [&'static str] {
    DEFAULT_MODEL_SLUGS
}

pub fn deepseek_default_model_slugs() -> &'static [&'static str] {
    DEEPSEEK_DEFAULT_MODEL_SLUGS
}

pub fn openai_default_model_slugs() -> &'static [&'static str] {
    OPENAI_DEFAULT_MODEL_SLUGS
}

pub fn default_models() -> Vec<ModelInfo> {
    vec![
        deepseek_model(
            "deepseek-v4-flash",
            "DeepSeek V4 Flash",
            "DeepSeek fast reasoning model with thinking mode.",
        ),
        deepseek_model(
            "deepseek-v4-pro",
            "DeepSeek V4 Pro",
            "DeepSeek flagship reasoning model with thinking mode.",
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

fn deepseek_model(slug: &str, display_name: &str, description: &str) -> ModelInfo {
    ModelInfo {
        slug: slug.to_string(),
        display_name: display_name.to_string(),
        description: Some(description.to_string()),
        context_window: Some(1_000_000),
        max_context_window: Some(1_000_000),
        auto_compact_token_limit: None,
        default_temperature: None,
        max_output_tokens: Some(384_000),
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
    fn default_model_slugs_are_backed_by_default_models() {
        let models = default_models();

        for slug in default_model_slugs() {
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
            assert!(model.reasoning_efforts.iter().any(|effort| effort == "max"));
        }
    }
}
