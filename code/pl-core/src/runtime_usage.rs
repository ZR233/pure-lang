use pl_protocol::{
    AgentRuntimeDelta, InferenceBillingRecord, InferenceTiming, InferenceTokenUsage,
    ModelPricingSnapshot, RuntimeCostAmount, RuntimeUsageSnapshot, ThreadPromptSnapshot,
    TokenUsageSnapshot,
};

use crate::tool::SubagentContext;
use pl_model::model::ModelInfo;
use pl_model::provider::EffectivePromptCachePolicy;
use pl_protocol::TokenUsage;

pub const ROOT_AGENT_ID: &str = "agent-root";
pub const ROOT_AGENT_PATH: &str = "/root";
pub const ROOT_AGENT_ROLE: &str = "root";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeAgentIdentity {
    pub agent_id: String,
    pub path: String,
    pub parent_path: Option<String>,
    pub role: String,
}

pub(crate) struct InferenceBillingInput<'a> {
    pub inference_id: String,
    pub provider_instance_id: &'a str,
    pub provider: &'a str,
    pub model: &'a str,
    pub usage: &'a TokenUsage,
    pub model_info: &'a ModelInfo,
    pub prompt_cache_policy: EffectivePromptCachePolicy,
    pub prompt: Option<&'a ThreadPromptSnapshot>,
    pub orchestration: pl_protocol::InferenceOrchestrationMetrics,
    pub timing: Option<InferenceTiming>,
    pub recorded_at: i64,
}

pub(crate) fn identity_for_subagent(
    active_subagent: Option<&SubagentContext>,
) -> RuntimeAgentIdentity {
    active_subagent.map_or_else(
        || RuntimeAgentIdentity {
            agent_id: ROOT_AGENT_ID.to_string(),
            path: ROOT_AGENT_PATH.to_string(),
            parent_path: None,
            role: ROOT_AGENT_ROLE.to_string(),
        },
        |subagent| RuntimeAgentIdentity {
            agent_id: subagent.id.clone(),
            path: subagent
                .agent_path
                .clone()
                .unwrap_or_else(|| ROOT_AGENT_PATH.to_string()),
            parent_path: subagent.parent_id.clone(),
            role: subagent.role.clone(),
        },
    )
}

pub fn token_usage_snapshot(usage: &TokenUsage) -> TokenUsageSnapshot {
    let cached_prompt_tokens = usage.cached_prompt_tokens.min(usage.prompt_tokens);
    let cache_write_tokens = usage
        .cache_write_tokens
        .min(usage.prompt_tokens.saturating_sub(cached_prompt_tokens));
    TokenUsageSnapshot {
        prompt_tokens: usage.prompt_tokens,
        completion_tokens: usage.completion_tokens,
        cached_prompt_tokens,
        cache_write_tokens,
        cache_miss_tokens: usage.prompt_tokens.saturating_sub(cached_prompt_tokens),
        reasoning_tokens: usage.reasoning_tokens,
        inference_count: 1,
        total_tokens: usage
            .total_tokens
            .max(usage.prompt_tokens.saturating_add(usage.completion_tokens)),
    }
}

pub(crate) fn inference_billing_record(input: InferenceBillingInput<'_>) -> InferenceBillingRecord {
    let reported_usage = InferenceTokenUsage {
        prompt_tokens: input.usage.prompt_tokens,
        cached_prompt_tokens: input.usage.cached_prompt_tokens,
        cache_write_tokens: input.usage.cache_write_tokens,
        completion_tokens: input.usage.completion_tokens,
        reasoning_tokens: input.usage.reasoning_tokens,
        total_tokens: input.usage.total_tokens,
    };
    let normalized_usage = reported_usage.normalized();
    let pricing = pricing_snapshot(input.model_info, input.prompt_cache_policy);
    let (estimated_costs, has_unpriced_usage) = cost_for_inference(&normalized_usage, &pricing);
    let estimated_cache_savings = cache_savings_for_inference(&normalized_usage, &pricing);
    InferenceBillingRecord {
        inference_id: input.inference_id,
        provider_instance_id: input.provider_instance_id.to_string(),
        provider: input.provider.to_string(),
        model: input.model.to_string(),
        context_window: input.model_info.resolved_context_window(),
        reported_usage,
        normalized_usage,
        pricing,
        estimated_costs,
        estimated_cache_savings,
        has_unpriced_usage,
        prompt_generation: input.prompt.map(|snapshot| snapshot.generation),
        prompt_cache_policy: input
            .prompt
            .map(|snapshot| snapshot.prompt_cache_policy.clone()),
        prefix_changed_reason: input.prompt.map(|snapshot| snapshot.prefix_changed_reason),
        orchestration: input.orchestration,
        timing: input.timing,
        recorded_at: input.recorded_at,
    }
}

fn pricing_snapshot(
    model_info: &ModelInfo,
    prompt_cache_policy: EffectivePromptCachePolicy,
) -> ModelPricingSnapshot {
    let reports_openai_cache_writes = model_info.capabilities.prompt_cache.cache_write_tokens
        && prompt_cache_policy
            == (EffectivePromptCachePolicy::OpenAiPromptCacheKey {
                cache_write_tokens: true,
            });
    let openai_cache_write_price = if reports_openai_cache_writes {
        model_info.input_price_per_mtok.map(|price| price * 1.25)
    } else {
        None
    };
    ModelPricingSnapshot {
        currency: model_info.currency.clone(),
        input_per_mtok: model_info.input_price_per_mtok,
        output_per_mtok: model_info.output_price_per_mtok,
        cache_read_per_mtok: model_info.cache_read_price_per_mtok,
        cache_write_per_mtok: model_info
            .cache_write_price_per_mtok
            .or(openai_cache_write_price),
    }
}

/// 模型 token usage 的宿主投影快照。
///
/// `TokenUsageSnapshot` 面向 pl-core runtime cost/trace，只保留成本计算需要的字段；
/// 宿主产品若需要展示 reasoning token 或 provider 返回的总数，应使用该类型，避免
/// 在产品层重复解释 `pl_protocol::TokenUsage`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelTokenUsageSnapshot {
    input_tokens: u64,
    cached_input_tokens: u64,
    cache_write_tokens: u64,
    output_tokens: u64,
    reasoning_output_tokens: u64,
    total_tokens: u64,
}

impl ModelTokenUsageSnapshot {
    pub fn input_tokens(&self) -> u64 {
        self.input_tokens
    }

    pub fn cached_input_tokens(&self) -> u64 {
        self.cached_input_tokens
    }

    pub fn output_tokens(&self) -> u64 {
        self.output_tokens
    }

    pub fn cache_write_tokens(&self) -> u64 {
        self.cache_write_tokens
    }

    pub fn reasoning_output_tokens(&self) -> u64 {
        self.reasoning_output_tokens
    }

    pub fn total_tokens(&self) -> u64 {
        self.total_tokens
    }
}

impl From<&TokenUsage> for ModelTokenUsageSnapshot {
    fn from(usage: &TokenUsage) -> Self {
        Self {
            input_tokens: usage.prompt_tokens,
            cached_input_tokens: usage.cached_prompt_tokens,
            cache_write_tokens: usage.cache_write_tokens,
            output_tokens: usage.completion_tokens,
            reasoning_output_tokens: usage.reasoning_tokens,
            total_tokens: usage.total_tokens,
        }
    }
}

pub(crate) fn agent_runtime_delta(
    identity: RuntimeAgentIdentity,
    billing: &InferenceBillingRecord,
) -> AgentRuntimeDelta {
    AgentRuntimeDelta {
        inference_id: billing.inference_id.clone(),
        agent_id: identity.agent_id,
        path: identity.path,
        parent_path: identity.parent_path,
        role: identity.role,
        model: billing.model.clone(),
        context_window: billing.context_window,
        usage: billing.normalized_usage.public_snapshot(),
        estimated_costs: billing.estimated_costs.clone(),
        estimated_cache_savings: billing.estimated_cache_savings.clone(),
        has_unpriced_usage: billing.has_unpriced_usage,
        prompt_generation: billing.prompt_generation,
        prompt_cache_policy: billing.prompt_cache_policy.clone(),
        prefix_changed_reason: billing.prefix_changed_reason,
        timing: billing.timing,
        updated_at: billing.recorded_at,
    }
}

pub fn aggregate_runtime_usage(
    session_id: &str,
    snapshots: impl IntoIterator<Item = RuntimeUsageSnapshot>,
) -> RuntimeUsageSnapshot {
    let mut aggregate = RuntimeUsageSnapshot {
        model: "unknown".to_string(),
        context_window: None,
        latest_context_tokens: 0,
        prompt_tokens: 0,
        completion_tokens: 0,
        cached_prompt_tokens: 0,
        cache_write_tokens: 0,
        cache_miss_tokens: 0,
        reasoning_tokens: 0,
        inference_count: 0,
        total_tokens: 0,
        estimated_costs: Vec::new(),
        estimated_cache_savings: Vec::new(),
        has_unpriced_usage: false,
        updated_at: 0,
    };

    for snapshot in snapshots {
        aggregate.prompt_tokens += snapshot.prompt_tokens;
        aggregate.completion_tokens += snapshot.completion_tokens;
        aggregate.cached_prompt_tokens += snapshot.cached_prompt_tokens;
        aggregate.cache_write_tokens += snapshot.cache_write_tokens;
        aggregate.cache_miss_tokens += snapshot.cache_miss_tokens;
        aggregate.reasoning_tokens += snapshot.reasoning_tokens;
        aggregate.inference_count += snapshot.inference_count;
        aggregate.total_tokens += snapshot.total_tokens;
        aggregate.has_unpriced_usage |= snapshot.has_unpriced_usage;
        merge_costs(&mut aggregate.estimated_costs, &snapshot.estimated_costs);
        merge_costs(
            &mut aggregate.estimated_cache_savings,
            &snapshot.estimated_cache_savings,
        );
        if snapshot.updated_at >= aggregate.updated_at {
            aggregate.model = snapshot.model;
            aggregate.context_window = snapshot.context_window;
            aggregate.latest_context_tokens = snapshot.latest_context_tokens;
            aggregate.updated_at = snapshot.updated_at;
        }
    }

    if aggregate.updated_at == 0 {
        aggregate.model = session_id.to_string();
    }
    aggregate
}

pub fn merge_costs(target: &mut Vec<RuntimeCostAmount>, incoming: &[RuntimeCostAmount]) {
    for cost in incoming {
        match target
            .iter_mut()
            .find(|existing| existing.currency == cost.currency)
        {
            Some(existing) => existing.amount += cost.amount,
            None => target.push(cost.clone()),
        }
    }
    target.sort_by(|left, right| {
        left.currency
            .cmp(&right.currency)
            .then_with(|| left.amount.total_cmp(&right.amount))
    });
}

fn estimate_cost(usage: &InferenceTokenUsage, pricing: &ModelPricingSnapshot) -> Option<f64> {
    let cached = usage.cached_prompt_tokens.min(usage.prompt_tokens);
    let written = usage
        .cache_write_tokens
        .min(usage.prompt_tokens.saturating_sub(cached));
    let ordinary_input = usage
        .prompt_tokens
        .saturating_sub(cached)
        .saturating_sub(written);
    let input_cost = if ordinary_input == 0 {
        0.0
    } else {
        ordinary_input as f64 * pricing.input_per_mtok?
    };
    let output_cost = if usage.completion_tokens == 0 {
        0.0
    } else {
        usage.completion_tokens as f64 * pricing.output_per_mtok?
    };
    let cache_cost = if cached == 0 {
        0.0
    } else {
        cached as f64 * pricing.cache_read_per_mtok?
    };
    let cache_write_cost = if written == 0 {
        0.0
    } else {
        written as f64 * pricing.cache_write_per_mtok?
    };
    Some((input_cost + output_cost + cache_cost + cache_write_cost) / 1_000_000.0)
}

fn cost_for_inference(
    usage: &InferenceTokenUsage,
    pricing: &ModelPricingSnapshot,
) -> (Vec<RuntimeCostAmount>, bool) {
    if usage.total_tokens == 0 {
        return (Vec::new(), false);
    }
    let Some(currency) = pricing.currency.as_ref().filter(|value| !value.is_empty()) else {
        return (Vec::new(), true);
    };
    let Some(amount) = estimate_cost(usage, pricing) else {
        return (Vec::new(), true);
    };
    (
        vec![RuntimeCostAmount {
            currency: currency.clone(),
            amount,
        }],
        false,
    )
}

fn cache_savings_for_inference(
    usage: &InferenceTokenUsage,
    pricing: &ModelPricingSnapshot,
) -> Vec<RuntimeCostAmount> {
    if usage.cached_prompt_tokens == 0 && usage.cache_write_tokens == 0 {
        return Vec::new();
    }
    let Some(currency) = pricing.currency.as_ref().filter(|value| !value.is_empty()) else {
        return Vec::new();
    };
    let Some(input_price) = pricing.input_per_mtok else {
        return Vec::new();
    };
    let input_usage = InferenceTokenUsage {
        prompt_tokens: usage.prompt_tokens,
        cached_prompt_tokens: usage.cached_prompt_tokens,
        cache_write_tokens: usage.cache_write_tokens,
        total_tokens: usage.prompt_tokens,
        ..InferenceTokenUsage::default()
    };
    let Some(actual_input_cost) = estimate_cost(&input_usage, pricing) else {
        return Vec::new();
    };
    let baseline_input_cost = usage.prompt_tokens as f64 * input_price / 1_000_000.0;
    vec![RuntimeCostAmount {
        currency: currency.clone(),
        amount: baseline_input_cost - actual_input_cost,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn openai_prompt_snapshot() -> ThreadPromptSnapshot {
        ThreadPromptSnapshot {
            scope: "simple:root".to_string(),
            generation: 1,
            provider: "OpenAI".to_string(),
            provider_hash: "provider".to_string(),
            model: "gpt-5.6-sol".to_string(),
            fixed_prefix_hash: "fixed".to_string(),
            fixed_prefix_section_hashes: Default::default(),
            request_properties_hash: "request".to_string(),
            tool_schema_hash: "tools".to_string(),
            context_hash: "context".to_string(),
            prompt_cache_policy: EffectivePromptCachePolicy::OpenAiPromptCacheKey {
                cache_write_tokens: true,
            }
            .label()
            .to_string(),
            prefix_changed_reason: pl_protocol::PromptPrefixChangedReason::Initial,
            updated_at: 1,
        }
    }

    #[test]
    fn model_token_usage_snapshot_preserves_reasoning_and_provider_total() {
        let usage = TokenUsage {
            prompt_tokens: 10,
            cached_prompt_tokens: 4,
            cache_write_tokens: 1,
            completion_tokens: 3,
            reasoning_tokens: 2,
            total_tokens: 13,
        };

        assert_eq!(
            ModelTokenUsageSnapshot::from(&usage),
            ModelTokenUsageSnapshot {
                input_tokens: 10,
                cached_input_tokens: 4,
                cache_write_tokens: 1,
                output_tokens: 3,
                reasoning_output_tokens: 2,
                total_tokens: 13,
            }
        );
    }

    #[test]
    fn token_usage_snapshot_clamps_cache_and_preserves_provider_total() {
        let snapshot = token_usage_snapshot(&TokenUsage {
            prompt_tokens: 10,
            cached_prompt_tokens: 20,
            cache_write_tokens: 3,
            completion_tokens: 3,
            reasoning_tokens: 2,
            total_tokens: 15,
        });

        assert_eq!(snapshot.cached_prompt_tokens, 10);
        assert_eq!(snapshot.cache_write_tokens, 0);
        assert_eq!(snapshot.cache_miss_tokens, 0);
        assert_eq!(snapshot.total_tokens, 15);
    }

    #[test]
    fn cost_only_requires_prices_for_non_zero_token_classes() {
        let pricing = ModelPricingSnapshot {
            input_per_mtok: Some(1.0),
            ..ModelPricingSnapshot::default()
        };
        assert_eq!(
            estimate_cost(
                &InferenceTokenUsage {
                    prompt_tokens: 10,
                    total_tokens: 10,
                    ..InferenceTokenUsage::default()
                },
                &pricing,
            ),
            Some(0.000_01)
        );
        assert_eq!(
            estimate_cost(
                &InferenceTokenUsage {
                    prompt_tokens: 10,
                    cached_prompt_tokens: 5,
                    total_tokens: 10,
                    ..InferenceTokenUsage::default()
                },
                &pricing,
            ),
            None
        );
    }

    #[test]
    fn openai_cache_write_price_defaults_to_one_point_two_five_times_input() {
        let mut model = ModelInfo::fallback("gpt-5.6-sol");
        model.currency = Some("USD".to_string());
        model.input_price_per_mtok = Some(1.0);
        model.output_price_per_mtok = Some(2.0);
        model.cache_read_price_per_mtok = Some(0.1);
        model.capabilities.prompt_cache.cache_write_tokens = true;
        let usage = TokenUsage {
            prompt_tokens: 1_000_000,
            cached_prompt_tokens: 0,
            cache_write_tokens: 1_000_000,
            completion_tokens: 0,
            reasoning_tokens: 0,
            total_tokens: 1_000_000,
        };

        let billing = inference_billing_record(InferenceBillingInput {
            inference_id: "inference-1".to_string(),
            provider_instance_id: "openai-primary",
            provider: "OpenAI",
            model: &model.slug,
            usage: &usage,
            model_info: &model,
            prompt_cache_policy: EffectivePromptCachePolicy::OpenAiPromptCacheKey {
                cache_write_tokens: true,
            },
            prompt: None,
            orchestration: Default::default(),
            timing: None,
            recorded_at: 1,
        });

        assert_eq!(billing.pricing.cache_write_per_mtok, Some(1.25));
        assert_eq!(billing.estimated_costs[0].amount, 1.25);
        assert_eq!(billing.estimated_cache_savings[0].amount, -0.25);
        assert_eq!(billing.prompt_generation, None);
    }

    #[test]
    fn explicit_cache_write_price_overrides_openai_default() {
        let mut model = ModelInfo::fallback("gpt-5.6-sol");
        model.currency = Some("USD".to_string());
        model.input_price_per_mtok = Some(1.0);
        model.output_price_per_mtok = Some(2.0);
        model.cache_write_price_per_mtok = Some(1.5);
        model.capabilities.prompt_cache.cache_write_tokens = true;
        let usage = TokenUsage {
            prompt_tokens: 1_000_000,
            cache_write_tokens: 1_000_000,
            total_tokens: 1_000_000,
            ..TokenUsage::default()
        };

        let prompt = openai_prompt_snapshot();
        let billing = inference_billing_record(InferenceBillingInput {
            inference_id: "inference-1".to_string(),
            provider_instance_id: "openai-primary",
            provider: "OpenAI",
            model: &model.slug,
            usage: &usage,
            model_info: &model,
            prompt_cache_policy: EffectivePromptCachePolicy::OpenAiPromptCacheKey {
                cache_write_tokens: true,
            },
            prompt: Some(&prompt),
            orchestration: Default::default(),
            timing: None,
            recorded_at: 1,
        });

        assert_eq!(billing.pricing.cache_write_per_mtok, Some(1.5));
        assert_eq!(billing.estimated_costs[0].amount, 1.5);
    }

    #[test]
    fn runtime_aggregate_keeps_currencies_separate_and_preserves_negative_savings() {
        let aggregate = aggregate_runtime_usage(
            "thread-1",
            [
                RuntimeUsageSnapshot {
                    model: "gpt-5.6-sol".to_string(),
                    context_window: Some(272_000),
                    latest_context_tokens: 10,
                    prompt_tokens: 100,
                    completion_tokens: 20,
                    cached_prompt_tokens: 40,
                    cache_write_tokens: 10,
                    cache_miss_tokens: 60,
                    reasoning_tokens: 5,
                    inference_count: 1,
                    total_tokens: 120,
                    estimated_costs: vec![RuntimeCostAmount {
                        currency: "USD".to_string(),
                        amount: 1.5,
                    }],
                    estimated_cache_savings: vec![RuntimeCostAmount {
                        currency: "USD".to_string(),
                        amount: -0.25,
                    }],
                    has_unpriced_usage: false,
                    updated_at: 1,
                },
                RuntimeUsageSnapshot {
                    model: "deepseek-v4-flash".to_string(),
                    context_window: Some(1_000_000),
                    latest_context_tokens: 20,
                    prompt_tokens: 200,
                    completion_tokens: 30,
                    cached_prompt_tokens: 100,
                    cache_write_tokens: 0,
                    cache_miss_tokens: 100,
                    reasoning_tokens: 0,
                    inference_count: 1,
                    total_tokens: 230,
                    estimated_costs: vec![RuntimeCostAmount {
                        currency: "CNY".to_string(),
                        amount: 2.0,
                    }],
                    estimated_cache_savings: vec![RuntimeCostAmount {
                        currency: "CNY".to_string(),
                        amount: 0.5,
                    }],
                    has_unpriced_usage: false,
                    updated_at: 2,
                },
            ],
        );

        assert_eq!(
            aggregate.estimated_costs,
            vec![
                RuntimeCostAmount {
                    currency: "CNY".to_string(),
                    amount: 2.0,
                },
                RuntimeCostAmount {
                    currency: "USD".to_string(),
                    amount: 1.5,
                },
            ]
        );
        assert_eq!(
            aggregate.estimated_cache_savings,
            vec![
                RuntimeCostAmount {
                    currency: "CNY".to_string(),
                    amount: 0.5,
                },
                RuntimeCostAmount {
                    currency: "USD".to_string(),
                    amount: -0.25,
                },
            ]
        );
        assert_eq!(aggregate.model, "deepseek-v4-flash");
        assert_eq!(aggregate.inference_count, 2);
    }
}
