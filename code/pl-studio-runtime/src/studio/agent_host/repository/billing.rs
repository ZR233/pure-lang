use std::collections::{BTreeMap, BTreeSet};

use pl_core::{AgentInferenceCommit, ThreadCommit, ThreadContextState};
use pl_protocol::{
    InferenceBillingAppend, RuntimeCostAmount, ThreadRuntimeSnapshot, ThreadRuntimeUsage,
    TurnBillingRecord,
};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter,
    QueryOrder,
};

use crate::PureError;
use crate::studio::StudioStore;
use crate::studio::entity::turn;

use super::store_error;

pub(super) async fn restore_billing(
    store: &StudioStore,
    thread_id: &str,
) -> Result<BTreeMap<String, TurnBillingRecord>, PureError> {
    let rows = turn::Entity::find()
        .filter(turn::Column::ThreadId.eq(thread_id))
        .filter(turn::Column::ModelJson.is_not_null())
        .order_by_asc(turn::Column::Ordinal)
        .all(store.database())
        .await
        .map_err(store_error)?;
    let mut inference_ids = BTreeSet::new();
    let mut billing_by_turn = BTreeMap::new();
    for row in rows {
        let Some(model_json) = row.model_json else {
            continue;
        };
        let billing: TurnBillingRecord = serde_json::from_str(&model_json)?;
        if billing.inferences.is_empty() {
            continue;
        }
        for inference in &billing.inferences {
            if !inference_ids.insert(inference.inference_id.clone()) {
                return Err(store_error(format!(
                    "duplicate inference {} while restoring Thread {thread_id}",
                    inference.inference_id
                )));
            }
        }
        billing_by_turn.insert(row.id, billing);
    }
    Ok(billing_by_turn)
}

pub(super) fn aggregate_billing_usage<'a>(
    billing: impl IntoIterator<Item = &'a TurnBillingRecord>,
) -> pl_model::TokenUsage {
    billing
        .into_iter()
        .fold(pl_model::TokenUsage::default(), |mut aggregate, turn| {
            let usage = turn.aggregate_usage();
            aggregate.prompt_tokens = aggregate.prompt_tokens.saturating_add(usage.prompt_tokens);
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
        })
}

pub(super) fn runtime_from_context(
    thread_id: &str,
    context: &ThreadContextState,
) -> Option<ThreadRuntimeSnapshot> {
    let inferences = context
        .billing_by_turn
        .values()
        .flat_map(|billing| billing.inferences.iter())
        .collect::<Vec<_>>();
    let latest = inferences.iter().copied().max_by(|left, right| {
        left.recorded_at
            .cmp(&right.recorded_at)
            .then_with(|| left.inference_id.cmp(&right.inference_id))
    })?;
    let mut costs = BTreeMap::<String, f64>::new();
    let mut cache_savings = BTreeMap::<String, f64>::new();
    for inference in &inferences {
        for cost in &inference.estimated_costs {
            *costs.entry(cost.currency.clone()).or_default() += cost.amount;
        }
        for saving in &inference.estimated_cache_savings {
            *cache_savings.entry(saving.currency.clone()).or_default() += saving.amount;
        }
    }
    let prompt_tokens = context.usage.prompt_tokens;
    let cached_prompt_tokens = context.usage.cached_prompt_tokens.min(prompt_tokens);
    let cache_hit_rate = (prompt_tokens > 0)
        .then_some((cached_prompt_tokens as f64 / prompt_tokens as f64).clamp(0.0, 1.0));
    Some(ThreadRuntimeSnapshot {
        thread_id: thread_id.to_string(),
        usage: ThreadRuntimeUsage {
            model: latest.model.clone(),
            context_window: latest.context_window,
            latest_context_tokens: context
                .last_context_tokens
                .unwrap_or(latest.normalized_usage.total_tokens),
            prompt_tokens,
            completion_tokens: context.usage.completion_tokens,
            cached_prompt_tokens,
            cache_write_tokens: context
                .usage
                .cache_write_tokens
                .min(prompt_tokens.saturating_sub(cached_prompt_tokens)),
            cache_miss_tokens: prompt_tokens.saturating_sub(cached_prompt_tokens),
            reasoning_tokens: context.usage.reasoning_tokens,
            inference_count: inferences.len() as u64,
            total_tokens: context.usage.total_tokens,
            cache_hit_rate,
            estimated_costs: costs
                .into_iter()
                .map(|(currency, amount)| RuntimeCostAmount { currency, amount })
                .collect(),
            estimated_cache_savings: cache_savings
                .into_iter()
                .map(|(currency, amount)| RuntimeCostAmount { currency, amount })
                .collect(),
            has_unpriced_usage: inferences
                .iter()
                .any(|inference| inference.has_unpriced_usage),
            prompt_generation: latest.prompt_generation,
            prompt_cache_policy: latest.prompt_cache_policy.clone(),
            prefix_changed_reason: latest.prefix_changed_reason,
            updated_at: latest.recorded_at,
        },
        todo: None,
        active_skills: Vec::new(),
        active_mcp_servers: Vec::new(),
        active_lsp_servers: Vec::new(),
        progress: None,
        mcp_health: None,
        tool_registry_revision: None,
        tool_catalog_hash: None,
        updated_at: latest.recorded_at,
    })
}

pub(super) fn authoritative_turn_usage(
    existing: Option<&turn::Model>,
    projected: Option<&pl_model::TokenUsage>,
) -> Result<pl_model::TokenUsage, PureError> {
    if let Some(model_json) = existing.and_then(|row| row.model_json.as_deref()) {
        let billing: TurnBillingRecord = serde_json::from_str(model_json)?;
        if !billing.inferences.is_empty() {
            return Ok(aggregate_billing_usage([&billing]));
        }
    }
    projected.cloned().map_or_else(
        || {
            existing.map_or_else(
                || Ok(pl_model::TokenUsage::default()),
                |row| serde_json::from_str(&row.usage_json).map_err(Into::into),
            )
        },
        Ok,
    )
}

pub(super) async fn persist_inference_billing(
    tx: &sea_orm::DatabaseTransaction,
    commit: &ThreadCommit,
) -> Result<(), PureError> {
    let Some(inference) = commit.facts.inference.as_ref() else {
        return Ok(());
    };
    validate_inference_commit(inference)?;
    let turn_id = commit
        .facts
        .turn_id
        .as_ref()
        .ok_or_else(|| store_error("inference billing requires a Turn id"))?;
    let row = turn::Entity::find_by_id(turn_id.to_string())
        .one(tx)
        .await
        .map_err(store_error)?
        .ok_or_else(|| store_error(format!("Turn {turn_id} is missing for inference billing")))?;
    if row.thread_id != commit.agent_id.as_str() {
        return Err(store_error(format!(
            "Turn {turn_id} belongs to another Thread"
        )));
    }
    let mut billing = row
        .model_json
        .as_deref()
        .map(serde_json::from_str::<TurnBillingRecord>)
        .transpose()?
        .unwrap_or_else(TurnBillingRecord::new);
    let append = billing
        .append(inference.billing.clone())
        .map_err(store_error)?;
    if append == InferenceBillingAppend::Identical {
        return Ok(());
    }
    billing.version = TurnBillingRecord::VERSION;
    let usage = aggregate_billing_usage([&billing]);
    let mut active = row.into_active_model();
    active.model_json = Set(Some(serde_json::to_string(&billing)?));
    active.usage_json = Set(serde_json::to_string(&usage)?);
    active.updated_at = Set(commit
        .next_state
        .snapshot
        .updated_at
        .max(inference.billing.recorded_at));
    active.update(tx).await.map_err(store_error)?;
    Ok(())
}

fn validate_inference_commit(inference: &AgentInferenceCommit) -> Result<(), PureError> {
    if inference.billing.inference_id != inference.runtime_delta.inference_id {
        return Err(store_error("billing and runtime inference ids differ"));
    }
    if inference.billing.normalized_usage.public_snapshot() != inference.runtime_delta.usage {
        return Err(store_error("billing and runtime usage differ"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, VecDeque};

    use pl_core::{
        AgentIdentity, AgentRoleId, AgentSnapshot, AgentState, CommitDurability,
        DurableCommitFacts, RunningAgentState, ThreadActorState, ThreadContextState, ThreadId,
        ThreadMutation, TurnId,
    };
    use pl_protocol::{
        AgentRuntimeDelta, InferenceBillingRecord, InferenceTokenUsage, ModelPricingSnapshot,
        RuntimeCostAmount,
    };
    use sea_orm::{ActiveModelTrait, EntityTrait, IntoActiveModel, TransactionTrait};

    use super::*;
    use crate::StudioMode;

    #[tokio::test]
    async fn inference_billing_is_idempotent_and_conflicts_roll_back_the_transaction() {
        let store = StudioStore::open_memory().await.unwrap();
        let project = store
            .upsert_project(std::env::temp_dir().join("billing-transaction-fixture"))
            .await
            .unwrap();
        let thread = store
            .create_thread(&project.id, "billing", StudioMode::Simple)
            .await
            .unwrap();
        let turn_id = "turn-billing";
        let turn_state = pl_protocol::TurnState::Running(pl_protocol::RunningTurnState::new(
            1,
            pl_protocol::TurnPhase::Thinking,
        ));
        turn::ActiveModel {
            id: Set(turn_id.to_string()),
            thread_id: Set(thread.id.clone()),
            ordinal: Set(0),
            revision: Set(0),
            state_json: Set(serde_json::to_string(&turn_state).unwrap()),
            model_json: Set(None),
            usage_json: Set(serde_json::to_string(&pl_model::TokenUsage::default()).unwrap()),
            metadata_json: Set(None),
            updated_at: Set(1),
            ..Default::default()
        }
        .insert(store.database())
        .await
        .unwrap();

        let billing = billing_record("inference-1", 10, 4, 3);
        commit_billing(&store, billing_commit(&thread.id, turn_id, billing.clone()))
            .await
            .unwrap();
        let inserted = turn::Entity::find_by_id(turn_id)
            .one(store.database())
            .await
            .unwrap()
            .unwrap();

        commit_billing(&store, billing_commit(&thread.id, turn_id, billing.clone()))
            .await
            .unwrap();
        let identical = turn::Entity::find_by_id(turn_id)
            .one(store.database())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(identical, inserted);

        let tx = store.database().begin().await.unwrap();
        let mut partial = identical.clone().into_active_model();
        partial.metadata_json = Set(Some("must roll back".to_string()));
        partial.update(&tx).await.unwrap();
        let conflict = billing_record("inference-1", 10, 4, 4);
        let error = persist_inference_billing(&tx, &billing_commit(&thread.id, turn_id, conflict))
            .await
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("conflicts with the durable billing record")
        );
        tx.rollback().await.unwrap();

        let after_conflict = turn::Entity::find_by_id(turn_id)
            .one(store.database())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(after_conflict, inserted);
    }

    #[test]
    fn terminal_projection_cannot_overwrite_authoritative_billing_usage() {
        let billing = TurnBillingRecord {
            version: TurnBillingRecord::VERSION,
            inferences: vec![billing_record("inference-1", 10, 4, 3)],
        };
        let row = turn::Model {
            id: "turn-1".to_string(),
            thread_id: "thread-1".to_string(),
            ordinal: 0,
            revision: 1,
            state_json: serde_json::to_string(&pl_protocol::TurnState::Running(
                pl_protocol::RunningTurnState::new(1, pl_protocol::TurnPhase::Thinking),
            ))
            .unwrap(),
            state_kind: "running".to_string(),
            model_json: Some(serde_json::to_string(&billing).unwrap()),
            usage_json: serde_json::to_string(&pl_model::TokenUsage::default()).unwrap(),
            metadata_json: None,
            updated_at: 1,
        };
        let projected = pl_model::TokenUsage {
            prompt_tokens: 999,
            completion_tokens: 999,
            total_tokens: 1_998,
            cached_prompt_tokens: 0,
            cache_write_tokens: 0,
            reasoning_tokens: 0,
        };

        let usage = authoritative_turn_usage(Some(&row), Some(&projected)).unwrap();

        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.cached_prompt_tokens, 4);
        assert_eq!(usage.completion_tokens, 3);
    }

    #[test]
    fn restart_projection_keeps_costs_separate_by_currency() {
        let mut cny = billing_record("cny", 10, 4, 3);
        cny.estimated_costs = vec![RuntimeCostAmount {
            currency: "CNY".to_string(),
            amount: 0.5,
        }];
        cny.estimated_cache_savings = vec![RuntimeCostAmount {
            currency: "CNY".to_string(),
            amount: 0.2,
        }];
        let mut usd = billing_record("usd", 20, 5, 4);
        usd.estimated_costs = vec![RuntimeCostAmount {
            currency: "USD".to_string(),
            amount: 0.25,
        }];
        usd.estimated_cache_savings = vec![RuntimeCostAmount {
            currency: "USD".to_string(),
            amount: -0.05,
        }];
        usd.has_unpriced_usage = true;
        usd.recorded_at = 2;
        let billing = BTreeMap::from([
            (
                "turn-cny".to_string(),
                TurnBillingRecord {
                    version: TurnBillingRecord::VERSION,
                    inferences: vec![cny],
                },
            ),
            (
                "turn-usd".to_string(),
                TurnBillingRecord {
                    version: TurnBillingRecord::VERSION,
                    inferences: vec![usd],
                },
            ),
        ]);
        let usage = aggregate_billing_usage(billing.values());
        let context = ThreadContextState {
            metadata: serde_json::Value::Null,
            session: pl_core::AgentSession::new(),
            usage,
            billing_by_turn: billing,
            last_context_tokens: Some(11),
            trace_sequence: 0,
            thread_revision: 0,
        };

        let runtime = runtime_from_context("thread-1", &context).unwrap();

        assert_eq!(
            runtime.usage.estimated_costs,
            vec![
                RuntimeCostAmount {
                    currency: "CNY".to_string(),
                    amount: 0.5,
                },
                RuntimeCostAmount {
                    currency: "USD".to_string(),
                    amount: 0.25,
                },
            ]
        );
        assert!(runtime.usage.has_unpriced_usage);
        assert_eq!(
            runtime.usage.estimated_cache_savings,
            vec![
                RuntimeCostAmount {
                    currency: "CNY".to_string(),
                    amount: 0.2,
                },
                RuntimeCostAmount {
                    currency: "USD".to_string(),
                    amount: -0.05,
                },
            ]
        );
        assert_eq!(runtime.usage.latest_context_tokens, 11);
        assert!(
            runtime
                .usage
                .cache_hit_rate
                .is_some_and(|rate| (0.0..=1.0).contains(&rate))
        );
    }

    async fn commit_billing(store: &StudioStore, commit: ThreadCommit) -> Result<(), PureError> {
        let tx = store.database().begin().await.map_err(store_error)?;
        persist_inference_billing(&tx, &commit).await?;
        tx.commit().await.map_err(store_error)
    }

    fn billing_commit(
        thread_id: &str,
        turn_id: &str,
        billing: InferenceBillingRecord,
    ) -> ThreadCommit {
        let thread_id = ThreadId::new(thread_id).unwrap();
        let turn_id = TurnId::new(turn_id).unwrap();
        let identity = AgentIdentity {
            id: thread_id.clone(),
            parent_id: None,
            role: AgentRoleId::new("executor").unwrap(),
            depth: 0,
        };
        let state = ThreadActorState {
            snapshot: AgentSnapshot {
                identity,
                state: AgentState::Running(RunningAgentState::new(turn_id.clone())),
                pending_inputs: 0,
                progress: None,
                last_turn: None,
                revision: 1,
                event_sequence: 1,
                updated_at: billing.recorded_at,
            },
            session: ThreadContextState::empty(),
            pending_inputs: VecDeque::new(),
            active_input: None,
        };
        let runtime_delta = AgentRuntimeDelta {
            inference_id: billing.inference_id.clone(),
            agent_id: thread_id.to_string(),
            path: thread_id.to_string(),
            parent_path: None,
            role: "executor".to_string(),
            model: billing.model.clone(),
            context_window: billing.context_window,
            usage: billing.normalized_usage.public_snapshot(),
            estimated_costs: billing.estimated_costs.clone(),
            estimated_cache_savings: billing.estimated_cache_savings.clone(),
            has_unpriced_usage: billing.has_unpriced_usage,
            prompt_generation: billing.prompt_generation,
            prompt_cache_policy: billing.prompt_cache_policy.clone(),
            prefix_changed_reason: billing.prefix_changed_reason,
            updated_at: billing.recorded_at,
        };
        ThreadCommit {
            agent_id: thread_id.clone(),
            durability: CommitDurability::Immediate,
            expected_revision: None,
            next_state: state,
            facts: DurableCommitFacts {
                thread_id: thread_id.clone(),
                turn_id: Some(turn_id),
                through_revision: 0,
                revision: 1,
                notifications: Vec::new(),
                turn_transition: None,
                context: None,
                projection_snapshot: None,
                runtime_events: Vec::new(),
                trace_events: Vec::new(),
                inference: Some(AgentInferenceCommit {
                    billing,
                    runtime_delta,
                }),
                submission: None,
            },
            mutation: ThreadMutation::ReplaceThread { thread_id },
        }
    }

    fn billing_record(
        inference_id: &str,
        prompt_tokens: u64,
        cached_prompt_tokens: u64,
        completion_tokens: u64,
    ) -> InferenceBillingRecord {
        let reported_usage = InferenceTokenUsage {
            prompt_tokens,
            cached_prompt_tokens,
            cache_write_tokens: 0,
            completion_tokens,
            reasoning_tokens: 0,
            total_tokens: prompt_tokens + completion_tokens,
        };
        let normalized_usage = reported_usage.normalized();
        InferenceBillingRecord {
            inference_id: inference_id.to_string(),
            provider: "DeepSeek".to_string(),
            model: "deepseek-v4-flash".to_string(),
            context_window: Some(1_000_000),
            reported_usage,
            normalized_usage,
            pricing: ModelPricingSnapshot {
                currency: Some("CNY".to_string()),
                input_per_mtok: Some(1.0),
                output_per_mtok: Some(2.0),
                cache_read_per_mtok: Some(0.02),
                cache_write_per_mtok: None,
            },
            estimated_costs: vec![RuntimeCostAmount {
                currency: "CNY".to_string(),
                amount: 0.000_1,
            }],
            estimated_cache_savings: Vec::new(),
            has_unpriced_usage: false,
            prompt_generation: None,
            prompt_cache_policy: None,
            prefix_changed_reason: None,
            orchestration: Default::default(),
            recorded_at: 1,
        }
    }
}
