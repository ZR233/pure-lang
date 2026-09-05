use pl_protocol::{
    AgentRuntimeDelta, InferenceAccounting, InferenceBillingRecord, InferenceTiming,
    RuntimeCostAmount, RuntimeUsageSnapshot, ThreadPromptSnapshot,
};

use crate::tool::SubagentContext;
use pl_model::model::ModelInfo;

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
    pub accounting: &'a InferenceAccounting,
    pub model_info: &'a ModelInfo,
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

pub(crate) fn inference_billing_record(input: InferenceBillingInput<'_>) -> InferenceBillingRecord {
    InferenceBillingRecord {
        inference_id: input.inference_id,
        provider_instance_id: input.provider_instance_id.to_string(),
        provider: input.provider.to_string(),
        model: input.model.to_string(),
        context_window: input.model_info.resolved_context_window(),
        accounting: input.accounting.clone(),
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

pub(crate) fn agent_runtime_delta(
    identity: RuntimeAgentIdentity,
    billing: &InferenceBillingRecord,
) -> AgentRuntimeDelta {
    AgentRuntimeDelta {
        has_incomplete_usage: billing.accounting.has_incomplete_usage(),
        context_tokens: billing.accounting.usage.known_total_tokens(),
        inference_id: billing.inference_id.clone(),
        agent_id: identity.agent_id,
        path: identity.path,
        parent_path: identity.parent_path,
        role: identity.role,
        model: billing.model.clone(),
        context_window: billing.context_window,
        usage: billing.accounting.usage.totals().public_snapshot(),
        estimated_costs: billing.accounting.estimated_costs().clone(),
        estimated_cache_savings: billing.accounting.estimated_cache_savings().clone(),
        has_unpriced_usage: billing.accounting.has_unpriced_usage(),
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
        has_incomplete_usage: false,
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
        aggregate.has_incomplete_usage |= snapshot.has_incomplete_usage;
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

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn runtime_aggregate_keeps_currencies_separate_and_preserves_negative_savings() {
        let aggregate = aggregate_runtime_usage(
            "thread-1",
            [
                RuntimeUsageSnapshot {
                    has_incomplete_usage: false,
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
                    has_incomplete_usage: false,
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
