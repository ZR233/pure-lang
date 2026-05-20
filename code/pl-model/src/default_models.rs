use crate::capabilities::ModelCapabilities;
use crate::model_info::{InputModality, ModelInfo, TruncationMode, TruncationPolicy};

pub(crate) const DEFAULT_MODEL: &str = "gpt-5.5";

const DEFAULT_MODEL_SLUGS: &[&str] = &["gpt-5.5", "gpt-5.4", "gpt-5.4-mini", "gpt-5.4-nano"];
const REASONING_EFFORTS: &[&str] = &["none", "low", "medium", "high", "xhigh"];

pub(crate) fn default_model_slugs() -> &'static [&'static str] {
    DEFAULT_MODEL_SLUGS
}

pub(crate) fn default_models() -> Vec<ModelInfo> {
    vec![
        model(
            "gpt-5.5",
            "GPT-5.5",
            "Frontier model for complex reasoning, coding, and professional work.",
            1_050_000,
        ),
        model(
            "gpt-5.4",
            "GPT-5.4",
            "Affordable frontier model for coding and professional work.",
            1_050_000,
        ),
        model(
            "gpt-5.4-mini",
            "GPT-5.4 Mini",
            "Efficient GPT-5.4-class model for coding, computer use, and subagents.",
            400_000,
        ),
        model(
            "gpt-5.4-nano",
            "GPT-5.4 Nano",
            "Lowest-cost GPT-5.4-class model for simple high-volume tasks.",
            400_000,
        ),
    ]
}

fn model(slug: &str, display_name: &str, description: &str, context_window: u64) -> ModelInfo {
    ModelInfo {
        slug: slug.to_string(),
        display_name: display_name.to_string(),
        description: Some(description.to_string()),
        context_window: Some(context_window),
        max_context_window: Some(context_window),
        auto_compact_token_limit: None,
        default_temperature: None,
        max_output_tokens: Some(128_000),
        reasoning_efforts: REASONING_EFFORTS
            .iter()
            .copied()
            .map(String::from)
            .collect(),
        capabilities: ModelCapabilities::STREAMING
            | ModelCapabilities::FUNCTION_CALLING
            | ModelCapabilities::VISION
            | ModelCapabilities::PARALLEL_TOOL_CALLS
            | ModelCapabilities::REASONING
            | ModelCapabilities::WEB_SEARCH,
        input_modalities: vec![InputModality::Text, InputModality::Image],
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
}
