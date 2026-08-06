use serde::{Deserialize, Serialize};

use crate::{PromptPrefixChangedReason, RuntimeCostAmount, TokenUsageSnapshot};

/// 向一个 Turn 追加 inference 计费记录的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InferenceBillingAppend {
    Inserted,
    Identical,
}

/// 模型调用发生时使用的价格快照。
///
/// 历史账单只能使用该快照重建，不能按当前 catalog 价格重新计算。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelPricingSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_per_mtok: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_per_mtok: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_per_mtok: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_per_mtok: Option<f64>,
}

/// 单次模型调用的 provider 原始 token 分类。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InferenceTokenUsage {
    pub prompt_tokens: u64,
    pub cached_prompt_tokens: u64,
    #[serde(default)]
    pub cache_write_tokens: u64,
    pub completion_tokens: u64,
    pub reasoning_tokens: u64,
    pub total_tokens: u64,
}

impl InferenceTokenUsage {
    pub fn normalized(&self) -> Self {
        let cached_prompt_tokens = self.cached_prompt_tokens.min(self.prompt_tokens);
        let cache_write_tokens = self
            .cache_write_tokens
            .min(self.prompt_tokens.saturating_sub(cached_prompt_tokens));
        Self {
            prompt_tokens: self.prompt_tokens,
            cached_prompt_tokens,
            cache_write_tokens,
            completion_tokens: self.completion_tokens,
            reasoning_tokens: self.reasoning_tokens,
            total_tokens: self
                .total_tokens
                .max(self.prompt_tokens.saturating_add(self.completion_tokens)),
        }
    }

    pub fn public_snapshot(&self) -> TokenUsageSnapshot {
        let normalized = self.normalized();
        TokenUsageSnapshot {
            prompt_tokens: normalized.prompt_tokens,
            completion_tokens: normalized.completion_tokens,
            cached_prompt_tokens: normalized.cached_prompt_tokens,
            cache_write_tokens: normalized.cache_write_tokens,
            cache_miss_tokens: normalized
                .prompt_tokens
                .saturating_sub(normalized.cached_prompt_tokens),
            reasoning_tokens: normalized.reasoning_tokens,
            inference_count: 1,
            total_tokens: normalized.total_tokens,
        }
    }
}

/// SQLite 中以 `inference_id` 幂等保存的单次计费记录。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InferenceBillingRecord {
    pub inference_id: String,
    pub provider: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    pub reported_usage: InferenceTokenUsage,
    pub normalized_usage: InferenceTokenUsage,
    pub pricing: ModelPricingSnapshot,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub estimated_costs: Vec<RuntimeCostAmount>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub estimated_cache_savings: Vec<RuntimeCostAmount>,
    #[serde(default)]
    pub has_unpriced_usage: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_generation: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_cache_policy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix_changed_reason: Option<PromptPrefixChangedReason>,
    pub recorded_at: i64,
}

/// 一个 Turn 的全部 inference 计费记录。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TurnBillingRecord {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub inferences: Vec<InferenceBillingRecord>,
}

impl TurnBillingRecord {
    pub const VERSION: u32 = 2;

    pub fn new() -> Self {
        Self {
            version: Self::VERSION,
            inferences: Vec::new(),
        }
    }

    pub fn aggregate_usage(&self) -> InferenceTokenUsage {
        self.inferences.iter().fold(
            InferenceTokenUsage::default(),
            |mut aggregate, inference| {
                let usage = &inference.normalized_usage;
                aggregate.prompt_tokens =
                    aggregate.prompt_tokens.saturating_add(usage.prompt_tokens);
                aggregate.cached_prompt_tokens = aggregate
                    .cached_prompt_tokens
                    .saturating_add(usage.cached_prompt_tokens);
                aggregate.cache_write_tokens = aggregate
                    .cache_write_tokens
                    .saturating_add(usage.cache_write_tokens);
                aggregate.completion_tokens = aggregate
                    .completion_tokens
                    .saturating_add(usage.completion_tokens);
                aggregate.reasoning_tokens = aggregate
                    .reasoning_tokens
                    .saturating_add(usage.reasoning_tokens);
                aggregate.total_tokens = aggregate.total_tokens.saturating_add(usage.total_tokens);
                aggregate
            },
        )
    }

    /// 按 `inference_id` 幂等追加；相同 id 的不同内容必须拒绝。
    pub fn append(
        &mut self,
        inference: InferenceBillingRecord,
    ) -> Result<InferenceBillingAppend, String> {
        let Some(existing) = self
            .inferences
            .iter()
            .find(|existing| existing.inference_id == inference.inference_id)
        else {
            self.inferences.push(inference);
            return Ok(InferenceBillingAppend::Inserted);
        };
        if existing == &inference {
            Ok(InferenceBillingAppend::Identical)
        } else {
            Err(format!(
                "inference {} conflicts with the durable billing record",
                inference.inference_id
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_normalization_clamps_cached_tokens() {
        let usage = InferenceTokenUsage {
            prompt_tokens: 10,
            cached_prompt_tokens: 20,
            cache_write_tokens: 3,
            completion_tokens: 3,
            reasoning_tokens: 2,
            total_tokens: 0,
        }
        .normalized();

        assert_eq!(usage.cached_prompt_tokens, 10);
        assert_eq!(usage.cache_write_tokens, 0);
        assert_eq!(usage.total_tokens, 13);
    }

    #[test]
    fn turn_billing_append_is_idempotent_and_rejects_conflicts() {
        let inference = InferenceBillingRecord {
            inference_id: "turn-1-inf-0".to_string(),
            provider: "DeepSeek".to_string(),
            model: "deepseek-v4-flash".to_string(),
            context_window: Some(1_000_000),
            reported_usage: InferenceTokenUsage {
                prompt_tokens: 10,
                cached_prompt_tokens: 4,
                cache_write_tokens: 2,
                completion_tokens: 3,
                reasoning_tokens: 2,
                total_tokens: 13,
            },
            normalized_usage: InferenceTokenUsage {
                prompt_tokens: 10,
                cached_prompt_tokens: 4,
                cache_write_tokens: 2,
                completion_tokens: 3,
                reasoning_tokens: 2,
                total_tokens: 13,
            },
            pricing: ModelPricingSnapshot::default(),
            estimated_costs: Vec::new(),
            estimated_cache_savings: Vec::new(),
            has_unpriced_usage: true,
            prompt_generation: Some(1),
            prompt_cache_policy: Some("implicitPrefix".to_string()),
            prefix_changed_reason: Some(PromptPrefixChangedReason::Initial),
            recorded_at: 1,
        };
        let mut billing = TurnBillingRecord::new();

        assert_eq!(
            billing.append(inference.clone()).unwrap(),
            InferenceBillingAppend::Inserted
        );
        assert_eq!(
            billing.append(inference.clone()).unwrap(),
            InferenceBillingAppend::Identical
        );

        let mut conflicting = inference;
        conflicting.normalized_usage.completion_tokens = 4;
        assert!(billing.append(conflicting).is_err());
        assert_eq!(billing.inferences.len(), 1);
    }

    #[test]
    fn version_one_json_defaults_version_two_fields() {
        let billing: TurnBillingRecord = serde_json::from_value(serde_json::json!({
            "version": 1,
            "inferences": [{
                "inferenceId": "inference-1",
                "provider": "DeepSeek",
                "model": "deepseek-v4-flash",
                "reportedUsage": {
                    "promptTokens": 10,
                    "cachedPromptTokens": 4,
                    "completionTokens": 3,
                    "reasoningTokens": 0,
                    "totalTokens": 13
                },
                "normalizedUsage": {
                    "promptTokens": 10,
                    "cachedPromptTokens": 4,
                    "completionTokens": 3,
                    "reasoningTokens": 0,
                    "totalTokens": 13
                },
                "pricing": {
                    "currency": "CNY",
                    "inputPerMtok": 1.0,
                    "outputPerMtok": 2.0,
                    "cacheReadPerMtok": 0.02
                },
                "estimatedCosts": [{"currency": "CNY", "amount": 0.0001}],
                "hasUnpricedUsage": false,
                "recordedAt": 1
            }]
        }))
        .unwrap();

        assert_eq!(billing.version, 1);
        assert_eq!(billing.inferences[0].reported_usage.cache_write_tokens, 0);
        assert_eq!(billing.inferences[0].pricing.cache_write_per_mtok, None);
        assert!(billing.inferences[0].estimated_cache_savings.is_empty());
        assert_eq!(billing.inferences[0].prompt_generation, None);
        assert_eq!(billing.aggregate_usage().total_tokens, 13);
    }
}
