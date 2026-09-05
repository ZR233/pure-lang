//! Typed usage conversion shared by streaming, fixtures and compaction.

use pl_protocol::UsageReport;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct ProviderTokenUsage {
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    completion_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    total_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_cache_hit_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_cache_miss_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_tokens_details: Option<TokenUsageDetails>,
    #[serde(skip_serializing_if = "Option::is_none")]
    input_tokens_details: Option<TokenUsageDetails>,
    #[serde(skip_serializing_if = "Option::is_none")]
    completion_tokens_details: Option<TokenUsageDetails>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_tokens_details: Option<TokenUsageDetails>,
}

impl ProviderTokenUsage {
    pub(crate) fn from_value(value: &serde_json::Value) -> Option<Self> {
        serde_json::from_value(value.clone()).ok()
    }

    pub(crate) fn to_responses_usage(&self) -> Option<UsageReport> {
        Some(UsageReport {
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            total_tokens: self.total_tokens,
            cache_read_tokens: self
                .input_tokens_details
                .as_ref()
                .and_then(|details| details.cached_tokens),
            cache_write_tokens: self
                .input_tokens_details
                .as_ref()
                .and_then(|details| details.cache_write_tokens),
            reasoning_tokens: self
                .output_tokens_details
                .as_ref()
                .and_then(|details| details.reasoning_tokens),
        })
    }

    pub(crate) fn to_chat_usage(&self) -> Option<UsageReport> {
        let cache_read_tokens = self.prompt_cache_hit_tokens.or_else(|| {
            self.prompt_tokens_details
                .as_ref()
                .and_then(|details| details.cached_tokens)
        });
        let input_tokens = self.prompt_tokens.or_else(|| {
            self.prompt_cache_hit_tokens?
                .checked_add(self.prompt_cache_miss_tokens?)
        });
        Some(UsageReport {
            input_tokens,
            output_tokens: self.completion_tokens,
            total_tokens: self.total_tokens,
            cache_read_tokens,
            cache_write_tokens: self
                .prompt_tokens_details
                .as_ref()
                .and_then(|details| details.cache_write_tokens),
            reasoning_tokens: self
                .completion_tokens_details
                .as_ref()
                .and_then(|details| details.reasoning_tokens),
        })
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct TokenUsageDetails {
    #[serde(skip_serializing_if = "Option::is_none")]
    cached_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_write_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_tokens: Option<u64>,
}
