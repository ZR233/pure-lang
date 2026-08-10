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

/// 单次 inference 及其直接工具批次的脱敏编排收益指标。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InferenceOrchestrationMetrics {
    #[serde(default)]
    pub tool_schema_estimated_tokens: u64,
    #[serde(default)]
    pub tool_result_estimated_tokens: u64,
    #[serde(default)]
    pub tool_calls: u64,
    #[serde(default)]
    pub parallel_candidates: u64,
    #[serde(default)]
    pub actual_parallel_calls: u64,
    #[serde(default)]
    pub tool_batch_elapsed_millis: u64,
    #[serde(default)]
    pub tool_execution_millis: u64,
    #[serde(default)]
    pub tool_critical_path_millis: u64,
    #[serde(default)]
    pub tool_cache_hits: u64,
    #[serde(default)]
    pub duplicate_suppressed: u64,
    #[serde(default)]
    pub tool_search_calls: u64,
    #[serde(default)]
    pub tool_search_loaded_tools: u64,
    #[serde(default)]
    pub program_count: u64,
    #[serde(default)]
    pub program_tool_calls: u64,
    #[serde(default)]
    pub transport_attempts: u64,
    #[serde(default)]
    pub continuation_attempts: u64,
    #[serde(default)]
    pub continuation_used: u64,
    #[serde(default)]
    pub continuation_invalid: u64,
    #[serde(default)]
    pub http_fallbacks: u64,
}

impl InferenceOrchestrationMetrics {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }

    pub fn parallel_saved_millis(&self) -> u64 {
        self.tool_execution_millis
            .saturating_sub(self.tool_batch_elapsed_millis)
    }
    pub fn merge(&mut self, other: &Self) {
        self.tool_schema_estimated_tokens = self
            .tool_schema_estimated_tokens
            .saturating_add(other.tool_schema_estimated_tokens);
        self.tool_result_estimated_tokens = self
            .tool_result_estimated_tokens
            .saturating_add(other.tool_result_estimated_tokens);
        self.tool_calls = self.tool_calls.saturating_add(other.tool_calls);
        self.parallel_candidates = self
            .parallel_candidates
            .saturating_add(other.parallel_candidates);
        self.actual_parallel_calls = self
            .actual_parallel_calls
            .saturating_add(other.actual_parallel_calls);
        self.tool_batch_elapsed_millis = self
            .tool_batch_elapsed_millis
            .saturating_add(other.tool_batch_elapsed_millis);
        self.tool_execution_millis = self
            .tool_execution_millis
            .saturating_add(other.tool_execution_millis);
        self.tool_critical_path_millis = self
            .tool_critical_path_millis
            .saturating_add(other.tool_critical_path_millis);
        self.tool_cache_hits = self.tool_cache_hits.saturating_add(other.tool_cache_hits);
        self.duplicate_suppressed = self
            .duplicate_suppressed
            .saturating_add(other.duplicate_suppressed);
        self.tool_search_calls = self
            .tool_search_calls
            .saturating_add(other.tool_search_calls);
        self.tool_search_loaded_tools = self
            .tool_search_loaded_tools
            .saturating_add(other.tool_search_loaded_tools);
        self.program_count = self.program_count.saturating_add(other.program_count);
        self.program_tool_calls = self
            .program_tool_calls
            .saturating_add(other.program_tool_calls);
        self.transport_attempts = self
            .transport_attempts
            .saturating_add(other.transport_attempts);
        self.continuation_attempts = self
            .continuation_attempts
            .saturating_add(other.continuation_attempts);
        self.continuation_used = self
            .continuation_used
            .saturating_add(other.continuation_used);
        self.continuation_invalid = self
            .continuation_invalid
            .saturating_add(other.continuation_invalid);
        self.http_fallbacks = self.http_fallbacks.saturating_add(other.http_fallbacks);
    }
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
    #[serde(
        default,
        skip_serializing_if = "InferenceOrchestrationMetrics::is_empty"
    )]
    pub orchestration: InferenceOrchestrationMetrics,
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
    pub const VERSION: u32 = 3;

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

    pub fn aggregate_orchestration(&self) -> InferenceOrchestrationMetrics {
        self.inferences.iter().fold(
            InferenceOrchestrationMetrics::default(),
            |mut aggregate, inference| {
                aggregate.merge(&inference.orchestration);
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
            orchestration: InferenceOrchestrationMetrics::default(),
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
    fn version_one_json_defaults_newer_fields() {
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
        assert!(billing.inferences[0].orchestration.is_empty());
        assert_eq!(billing.aggregate_usage().total_tokens, 13);
    }

    #[test]
    fn turn_billing_aggregates_orchestration_metrics() {
        let mut first = InferenceOrchestrationMetrics {
            tool_calls: 2,
            tool_execution_millis: 30,
            tool_batch_elapsed_millis: 20,
            ..InferenceOrchestrationMetrics::default()
        };
        first.merge(&InferenceOrchestrationMetrics {
            tool_cache_hits: 1,
            ..InferenceOrchestrationMetrics::default()
        });
        let mut inference = InferenceBillingRecord {
            inference_id: "inference-1".to_string(),
            provider: "OpenAI".to_string(),
            model: "gpt-5.6-sol".to_string(),
            context_window: None,
            reported_usage: InferenceTokenUsage::default(),
            normalized_usage: InferenceTokenUsage::default(),
            pricing: ModelPricingSnapshot::default(),
            estimated_costs: Vec::new(),
            estimated_cache_savings: Vec::new(),
            has_unpriced_usage: false,
            prompt_generation: None,
            prompt_cache_policy: None,
            prefix_changed_reason: None,
            orchestration: first,
            recorded_at: 1,
        };
        let mut billing = TurnBillingRecord::new();
        billing.inferences.push(inference.clone());
        inference.inference_id = "inference-2".to_string();
        billing.inferences.push(inference);

        let aggregate = billing.aggregate_orchestration();
        assert_eq!(aggregate.tool_calls, 4);
        assert_eq!(aggregate.tool_cache_hits, 2);
        assert_eq!(aggregate.parallel_saved_millis(), 20);
    }
}
