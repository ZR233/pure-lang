//! token 用量与 reasoning 配置。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    #[serde(default)]
    pub cached_prompt_tokens: u64,
    #[serde(default)]
    pub cache_write_tokens: u64,
    #[serde(default)]
    pub reasoning_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningConfig {
    pub effort: Option<String>,
    pub summary: Option<ReasoningSummary>,
}

impl ReasoningConfig {
    pub fn is_enabled(&self) -> bool {
        !matches!(
            self.effort.as_deref(),
            None | Some("") | Some("none") | Some("disabled")
        ) || !matches!(self.summary, None | Some(ReasoningSummary::Disabled))
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningSummary {
    Auto,
    Enabled,
    Disabled,
}
