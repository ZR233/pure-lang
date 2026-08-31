//! OpenAI-compatible usage aliases 的唯一 typed normalizer。

use serde::Deserialize;

use crate::completion::TokenUsage;

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ProviderTokenUsage {
    prompt_tokens: Option<u64>,
    input_tokens: Option<u64>,
    completion_tokens: Option<u64>,
    output_tokens: Option<u64>,
    total_tokens: Option<u64>,
    prompt_cache_hit_tokens: Option<u64>,
    cached_prompt_tokens: Option<u64>,
    prompt_tokens_details: Option<TokenUsageDetails>,
    input_tokens_details: Option<TokenUsageDetails>,
    completion_tokens_details: Option<TokenUsageDetails>,
    output_tokens_details: Option<TokenUsageDetails>,
}

impl ProviderTokenUsage {
    pub(super) fn from_value(value: &serde_json::Value) -> Option<Self> {
        serde_json::from_value(value.clone()).ok()
    }

    pub(super) fn to_responses_usage(&self) -> Option<TokenUsage> {
        self.normalize(self.input_tokens?, self.output_tokens?)
    }

    pub(super) fn to_chat_usage(&self) -> Option<TokenUsage> {
        self.normalize(
            self.prompt_tokens.or(self.input_tokens)?,
            self.completion_tokens.or(self.output_tokens)?,
        )
    }

    fn normalize(&self, prompt_tokens: u64, completion_tokens: u64) -> Option<TokenUsage> {
        Some(TokenUsage {
            prompt_tokens,
            completion_tokens,
            total_tokens: self.total_tokens.unwrap_or(0),
            cached_prompt_tokens: self.cached_prompt_tokens(),
            cache_write_tokens: self.cache_write_tokens(),
            reasoning_tokens: self.reasoning_tokens(),
        })
    }

    fn cached_prompt_tokens(&self) -> u64 {
        self.prompt_cache_hit_tokens
            .or(self.cached_prompt_tokens)
            .or_else(|| {
                self.input_tokens_details
                    .as_ref()
                    .and_then(TokenUsageDetails::cached)
            })
            .or_else(|| {
                self.prompt_tokens_details
                    .as_ref()
                    .and_then(TokenUsageDetails::cached)
            })
            .unwrap_or(0)
    }

    fn reasoning_tokens(&self) -> u64 {
        self.output_tokens_details
            .as_ref()
            .and_then(TokenUsageDetails::reasoning)
            .or_else(|| {
                self.completion_tokens_details
                    .as_ref()
                    .and_then(TokenUsageDetails::reasoning)
            })
            .unwrap_or(0)
    }

    fn cache_write_tokens(&self) -> u64 {
        self.input_tokens_details
            .as_ref()
            .and_then(TokenUsageDetails::cache_write)
            .or_else(|| {
                self.prompt_tokens_details
                    .as_ref()
                    .and_then(TokenUsageDetails::cache_write)
            })
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone, Deserialize)]
struct TokenUsageDetails {
    cached_tokens: Option<u64>,
    cache_read_tokens: Option<u64>,
    cached_input_tokens: Option<u64>,
    cache_write_tokens: Option<u64>,
    reasoning_tokens: Option<u64>,
}

impl TokenUsageDetails {
    fn cached(&self) -> Option<u64> {
        self.cached_tokens
            .or(self.cache_read_tokens)
            .or(self.cached_input_tokens)
    }

    fn reasoning(&self) -> Option<u64> {
        self.reasoning_tokens
    }

    fn cache_write(&self) -> Option<u64> {
        self.cache_write_tokens
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_cache_aliases_precede_nested_details() {
        let usage = ProviderTokenUsage::from_value(&serde_json::json!({
            "input_tokens": 100,
            "output_tokens": 20,
            "cached_prompt_tokens": 55,
            "input_tokens_details": {"cached_tokens": 20}
        }))
        .unwrap()
        .to_responses_usage()
        .unwrap();

        assert_eq!(usage.cached_prompt_tokens, 55);
    }
}
