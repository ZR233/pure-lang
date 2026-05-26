use serde::Deserialize;
use serde::Serialize;

use crate::capabilities::ModelCapabilities;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub slug: String,
    pub display_name: String,
    pub description: Option<String>,

    pub context_window: Option<u64>,
    pub max_context_window: Option<u64>,
    pub auto_compact_token_limit: Option<u64>,

    pub default_temperature: Option<f32>,
    pub max_output_tokens: Option<u64>,
    pub currency: Option<String>,
    pub input_price_per_mtok: Option<f64>,
    pub output_price_per_mtok: Option<f64>,
    pub cache_read_price_per_mtok: Option<f64>,
    #[serde(default)]
    pub reasoning_efforts: Vec<String>,

    pub capabilities: ModelCapabilities,
    pub input_modalities: Vec<InputModality>,

    pub truncation_policy: TruncationPolicy,

    pub base_instructions: String,

    #[serde(skip)]
    pub used_fallback: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InputModality {
    Text,
    Image,
    Audio,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TruncationPolicy {
    pub mode: TruncationMode,
    pub limit: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TruncationMode {
    Bytes,
    Tokens,
}

impl ModelInfo {
    pub fn resolved_context_window(&self) -> Option<u64> {
        self.context_window.or(self.max_context_window)
    }

    pub fn resolved_auto_compact_limit(&self) -> Option<u64> {
        let context = self.resolved_context_window()?;
        let default_limit = (context * 90) / 100;
        Some(
            self.auto_compact_token_limit
                .map_or(default_limit, |limit| limit.min(default_limit)),
        )
    }

    pub fn fallback(slug: &str) -> Self {
        Self {
            slug: slug.to_string(),
            display_name: slug.to_string(),
            description: None,
            context_window: Some(128_000),
            max_context_window: Some(128_000),
            auto_compact_token_limit: None,
            default_temperature: Some(0.3),
            max_output_tokens: Some(4096),
            currency: None,
            input_price_per_mtok: None,
            output_price_per_mtok: None,
            cache_read_price_per_mtok: None,
            reasoning_efforts: Vec::new(),
            capabilities: ModelCapabilities::STREAMING | ModelCapabilities::FUNCTION_CALLING,
            input_modalities: vec![InputModality::Text],
            truncation_policy: TruncationPolicy {
                mode: TruncationMode::Bytes,
                limit: 10_000,
            },
            base_instructions: String::new(),
            used_fallback: true,
        }
    }
}
