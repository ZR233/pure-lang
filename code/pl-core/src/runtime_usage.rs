use pl_model::{ModelInfo, TokenUsage};
use pl_protocol::{
    AgentRuntimeDelta, InferenceBillingRecord, InferenceTokenUsage, ModelPricingSnapshot,
    RuntimeCostAmount, RuntimeUsageSnapshot, TokenUsageSnapshot,
};

use crate::tool::SubagentContext;

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
    TokenUsageSnapshot {
        prompt_tokens: usage.prompt_tokens,
        completion_tokens: usage.completion_tokens,
        cached_prompt_tokens,
        total_tokens: usage
            .total_tokens
            .max(usage.prompt_tokens.saturating_add(usage.completion_tokens)),
    }
}

pub(crate) fn inference_billing_record(
    inference_id: String,
    provider: String,
    model: String,
    usage: &TokenUsage,
    model_info: &ModelInfo,
    recorded_at: i64,
) -> InferenceBillingRecord {
    let reported_usage = InferenceTokenUsage {
        prompt_tokens: usage.prompt_tokens,
        cached_prompt_tokens: usage.cached_prompt_tokens,
        completion_tokens: usage.completion_tokens,
        reasoning_tokens: usage.reasoning_tokens,
        total_tokens: usage.total_tokens,
    };
    let normalized_usage = reported_usage.normalized();
    let pricing = ModelPricingSnapshot {
        currency: model_info.currency.clone(),
        input_per_mtok: model_info.input_price_per_mtok,
        output_per_mtok: model_info.output_price_per_mtok,
        cache_read_per_mtok: model_info.cache_read_price_per_mtok,
    };
    let (estimated_costs, has_unpriced_usage) =
        cost_for_usage(&normalized_usage.public_snapshot(), Some(model_info));
    InferenceBillingRecord {
        inference_id,
        provider,
        model,
        context_window: model_info.resolved_context_window(),
        reported_usage,
        normalized_usage,
        pricing,
        estimated_costs,
        has_unpriced_usage,
        recorded_at,
    }
}

/// 模型 token usage 的宿主投影快照。
///
/// `TokenUsageSnapshot` 面向 pl-core runtime cost/trace，只保留成本计算需要的字段；
/// 宿主产品若需要展示 reasoning token 或 provider 返回的总数，应使用该类型，避免
/// 在产品层重复解释 `pl_model::TokenUsage`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelTokenUsageSnapshot {
    input_tokens: u64,
    cached_input_tokens: u64,
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

    pub fn reasoning_output_tokens(&self) -> u64 {
        self.reasoning_output_tokens
    }

    pub fn total_tokens(&self) -> u64 {
        self.total_tokens
    }

    pub fn from_model_usage(usage: &TokenUsage) -> Self {
        Self {
            input_tokens: usage.prompt_tokens,
            cached_input_tokens: usage.cached_prompt_tokens,
            output_tokens: usage.completion_tokens,
            reasoning_output_tokens: usage.reasoning_tokens,
            total_tokens: usage.total_tokens,
        }
    }
}

pub fn cost_for_usage(
    usage: &TokenUsageSnapshot,
    model: Option<&ModelInfo>,
) -> (Vec<RuntimeCostAmount>, bool) {
    if usage.total_tokens == 0 {
        return (Vec::new(), false);
    }

    let Some(model) = model else {
        return (Vec::new(), true);
    };
    let Some(currency) = model.currency.as_ref().filter(|value| !value.is_empty()) else {
        return (Vec::new(), true);
    };
    let Some(amount) = estimate_cost(
        usage.prompt_tokens,
        usage.completion_tokens,
        usage.cached_prompt_tokens,
        model.input_price_per_mtok,
        model.output_price_per_mtok,
        model.cache_read_price_per_mtok,
    ) else {
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

pub(crate) fn agent_runtime_delta(
    inference_id: String,
    identity: RuntimeAgentIdentity,
    model: &ModelInfo,
    usage: TokenUsageSnapshot,
    updated_at: i64,
) -> AgentRuntimeDelta {
    let (estimated_costs, has_unpriced_usage) = cost_for_usage(&usage, Some(model));
    AgentRuntimeDelta {
        inference_id,
        agent_id: identity.agent_id,
        path: identity.path,
        parent_path: identity.parent_path,
        role: identity.role,
        model: model.slug.clone(),
        context_window: model.resolved_context_window(),
        usage,
        estimated_costs,
        has_unpriced_usage,
        updated_at,
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
        total_tokens: 0,
        estimated_costs: Vec::new(),
        has_unpriced_usage: false,
        updated_at: 0,
    };

    for snapshot in snapshots {
        aggregate.prompt_tokens += snapshot.prompt_tokens;
        aggregate.completion_tokens += snapshot.completion_tokens;
        aggregate.cached_prompt_tokens += snapshot.cached_prompt_tokens;
        aggregate.total_tokens += snapshot.total_tokens;
        aggregate.has_unpriced_usage |= snapshot.has_unpriced_usage;
        merge_costs(&mut aggregate.estimated_costs, &snapshot.estimated_costs);
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
    aggregate.estimated_costs.sort_by(|left, right| {
        left.currency
            .cmp(&right.currency)
            .then_with(|| left.amount.total_cmp(&right.amount))
    });
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
}

pub(crate) fn estimate_cost(
    prompt_tokens: u64,
    completion_tokens: u64,
    cached_prompt_tokens: u64,
    input_price_per_mtok: Option<f64>,
    output_price_per_mtok: Option<f64>,
    cache_read_price_per_mtok: Option<f64>,
) -> Option<f64> {
    let cached = cached_prompt_tokens.min(prompt_tokens);
    let uncached_input = prompt_tokens.saturating_sub(cached);
    let input_cost = if uncached_input == 0 {
        0.0
    } else {
        uncached_input as f64 * input_price_per_mtok?
    };
    let output_cost = if completion_tokens == 0 {
        0.0
    } else {
        completion_tokens as f64 * output_price_per_mtok?
    };
    let cache_cost = if cached == 0 {
        0.0
    } else {
        cached as f64 * cache_read_price_per_mtok?
    };
    Some((input_cost + output_cost + cache_cost) / 1_000_000.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn model_token_usage_snapshot_preserves_reasoning_and_provider_total() {
        let usage = TokenUsage {
            prompt_tokens: 10,
            cached_prompt_tokens: 4,
            completion_tokens: 3,
            reasoning_tokens: 2,
            total_tokens: 13,
        };

        assert_eq!(
            ModelTokenUsageSnapshot::from_model_usage(&usage),
            ModelTokenUsageSnapshot {
                input_tokens: 10,
                cached_input_tokens: 4,
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
            completion_tokens: 3,
            reasoning_tokens: 2,
            total_tokens: 15,
        });

        assert_eq!(snapshot.cached_prompt_tokens, 10);
        assert_eq!(snapshot.total_tokens, 15);
    }

    #[test]
    fn cost_only_requires_prices_for_non_zero_token_classes() {
        assert_eq!(
            estimate_cost(10, 0, 0, Some(1.0), None, None),
            Some(0.000_01)
        );
        assert_eq!(estimate_cost(10, 0, 5, Some(1.0), None, None), None);
    }
}
