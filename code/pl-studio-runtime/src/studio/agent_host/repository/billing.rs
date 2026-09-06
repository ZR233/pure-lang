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
) -> pl_protocol::InferenceTokenUsage {
    billing.into_iter().fold(
        pl_protocol::InferenceTokenUsage::default(),
        |mut aggregate, turn| {
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
        },
    )
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
        for cost in &inference.accounting.estimated_costs() {
            *costs.entry(cost.currency.clone()).or_default() += cost.amount;
        }
        for saving in &inference.accounting.estimated_cache_savings() {
            *cache_savings.entry(saving.currency.clone()).or_default() += saving.amount;
        }
    }
    let prompt_tokens = context.usage.prompt_tokens;
    let cached_prompt_tokens = context.usage.cached_prompt_tokens;
    let has_incomplete_usage = inferences
        .iter()
        .any(|inference| inference.accounting.has_incomplete_usage());
    let cache_hit_rate = (prompt_tokens > 0 && !has_incomplete_usage)
        .then_some(cached_prompt_tokens as f64 / prompt_tokens as f64);
    Some(ThreadRuntimeSnapshot {
        thread_id: thread_id.to_string(),
        usage: ThreadRuntimeUsage {
            has_incomplete_usage,
            model: latest.model.clone(),
            context_window: latest.context_window,
            latest_context_tokens: context
                .last_context_tokens
                .or_else(|| {
                    inferences
                        .iter()
                        .rev()
                        .find_map(|inference| inference.accounting.usage.known_total_tokens())
                })
                .unwrap_or(0),
            prompt_tokens,
            completion_tokens: context.usage.completion_tokens,
            cached_prompt_tokens,
            cache_write_tokens: context.usage.cache_write_tokens,
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
                .any(|inference| inference.accounting.has_unpriced_usage()),
            prompt_generation: latest.prompt_generation,
            prompt_cache_policy: latest.prompt_cache_policy.clone(),
            prefix_changed_reason: latest.prefix_changed_reason,
            updated_at: latest.recorded_at,
        },
        turn_completion_tokens: 0,
        turn_decode_millis: 0,
        todo: None,
        active_skills: Vec::new(),
        active_mcp_servers: Vec::new(),
        active_lsp_servers: Vec::new(),
        progress: None,
        mcp_health: None,
        workflow: context.session.workflow().map(Into::into),
        updated_at: latest.recorded_at,
    })
}

pub(super) fn authoritative_turn_usage(
    existing: Option<&turn::Model>,
    projected: Option<&pl_protocol::InferenceTokenUsage>,
) -> Result<pl_protocol::InferenceTokenUsage, PureError> {
    if let Some(model_json) = existing.and_then(|row| row.model_json.as_deref()) {
        let billing: TurnBillingRecord = serde_json::from_str(model_json)?;
        if !billing.inferences.is_empty() {
            return Ok(aggregate_billing_usage([&billing]));
        }
    }
    projected.cloned().map_or_else(
        || {
            existing.map_or_else(
                || Ok(pl_protocol::InferenceTokenUsage::default()),
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
    if inference
        .billing
        .accounting
        .usage
        .totals()
        .public_snapshot()
        != inference.runtime_delta.usage
    {
        return Err(store_error("billing and runtime usage differ"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, VecDeque};

    use pl_core::{
        AgentIdentity, AgentRoleId, AgentSnapshot, AgentState, DurableCommitFacts,
        PersistenceClass, RunningAgentState, ThreadActorState, ThreadContextState, ThreadId,
        ThreadMutation, TurnId,
    };
    use pl_protocol::{
        AgentRuntimeDelta, InferenceBillingRecord, ModelPricingSnapshot, RuntimeCostAmount,
    };
    use sea_orm::{ActiveModelTrait, EntityTrait, IntoActiveModel, TransactionTrait};

    use super::*;
    use crate::ThreadModeId;

    #[tokio::test]
    async fn compatible_provider_settings_execute_and_persist_immutable_billing() {
        let store = StudioStore::open_memory().await.unwrap();
        let project = store
            .upsert_project(std::env::temp_dir().join("billing-transaction-fixture"))
            .await
            .unwrap();
        let thread = store
            .create_thread(&project.id, "billing", ThreadModeId::simple())
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
            usage_json: Set(
                serde_json::to_string(&pl_protocol::InferenceTokenUsage::default()).unwrap(),
            ),
            metadata_json: Set(None),
            updated_at: Set(1),
            ..Default::default()
        }
        .insert(store.database())
        .await
        .unwrap();

        let (base_url, server) = compatible_model_server().await;
        let edit = crate::ProviderSettingsEdit {
            default_provider: Some("local".into()),
            providers: vec![crate::ProviderEdit {
                key: "local".into(),
                original_key: None,
                preset: Some(crate::ProviderPresetId::new("openai-compatible").unwrap()),
                name: "Local model".into(),
                base_url: Some(base_url),
                bearer_token: None,
                pricing_mode: pl_protocol::PricingMode::Catalog,
                default_model: "local-coder".into(),
                custom_models: vec![crate::ProviderModelEdit {
                    slug: "local-coder".into(),
                    display_name: String::new(),
                    protocol: pl_model::provider::ProviderWireProtocol::ChatCompletions,
                    context_window: 32_000,
                    max_output_tokens: 4096,
                }],
                model_connection_modes: Default::default(),
            }],
            roles: Vec::new(),
        };
        let mut config = edit.to_config(&crate::StudioConfig::default()).unwrap();
        let provider = config.models.providers.values_mut().next().unwrap();
        let crate::ProviderModelCatalogConfig::Explicit { models, .. } = &mut provider.catalog
        else {
            panic!("custom catalog");
        };
        models[0].pricing = pl_model::model::ModelPricing::published(
            "CNY",
            vec![pl_model::model::TokenPriceTier::flat(
                1.5,
                4.5,
                Some(0.05),
                None,
            )],
            "https://fixture.example/pricing",
        );
        config.validate().unwrap();
        let route = config.resolve_role(crate::StudioRole::Executor).unwrap();
        let client = pl_core::ModelTurnClient::from_route(&route).unwrap();
        let mut session = pl_core::AgentSession::new();
        session.push_user_prompt("Complete the configured model task".to_string());
        let response = client
            .complete(
                &session,
                pl_core::ModelTurnRequest::new(),
                pl_core::ModelTurnOptions::default(),
            )
            .await
            .unwrap();
        server.await.unwrap();
        assert_eq!(
            response
                .output()
                .iter()
                .filter_map(|item| item.as_message())
                .collect::<String>(),
            "ready"
        );
        let mut billing = billing_record("inference-1", 10, 4, 3);
        billing.provider_instance_id = route.provider_id.to_string();
        billing.provider = route.endpoint.name;
        billing.model = response.model().to_owned();
        billing.accounting = response.accounting().clone();
        let pl_protocol::PricingOutcome::Estimated { cost, .. } = &billing.accounting.pricing
        else {
            panic!("reported inference must be priced");
        };
        assert!((cost.amount - 0.000_022_7).abs() < 1e-12);

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
        let mut conflict = billing.clone();
        conflict.accounting.usage.output_tokens = Some(4);
        conflict.accounting.usage.total_tokens = Some(14);
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

    #[tokio::test]
    async fn failed_save_retries_messages_tool_results_billing_and_submission_once() {
        use super::super::write_behind::ThreadWriteBehindWriter;
        use sea_orm::ConnectionTrait;
        let store = StudioStore::open_memory().await.unwrap();
        let project = store
            .upsert_project(std::env::temp_dir().join("save-facts"))
            .await
            .unwrap();
        let thread = store
            .create_thread(&project.id, "facts", ThreadModeId::simple())
            .await
            .unwrap();
        let billing = billing_record("saved-inference", 10, 4, 3);
        let mut commit = billing_commit(&thread.id, "saved-turn", billing.clone());
        let session = &mut commit.next_state.session.session;
        session.push_user_prompt("Keep this message".into());
        session.push_tool_result(
            pl_protocol::ToolResultRecord {
                item_id: "tool-item".into(),
                call_id: "call-1".into(),
                name: "exec".into(),
                kind: pl_protocol::ToolCallKind::Function,
            },
            "Keep this tool output".into(),
            "{}".into(),
        );
        let transcript = session.snapshot().transcript;
        commit.facts.context = Some(pl_core::ThreadContextMutation::Append {
            items: transcript.clone(),
        });
        let submission = pl_core::ProgressSubmissionCommit {
            report: pl_protocol::AgentProgressReport {
                stage: pl_protocol::AgentProgressStage::Verifying,
                summary: "Keep this report".into(),
                next_step: "Finish".into(),
                revision: 1,
            },
            detail: Some("Report detail".into()),
            created_at: 1,
        };
        commit.facts.submission = Some(submission.clone());
        store.database().execute_unprepared("CREATE TRIGGER fail_facts BEFORE UPDATE ON threads BEGIN SELECT RAISE(ABORT, 'disk i/o error'); END").await.unwrap();
        let writer = ThreadWriteBehindWriter::new(store.clone());
        let attachment = crate::studio::AttachmentRecord {
            id: "saved-attachment".into(),
            thread_id: thread.id.clone(),
            modality: pl_protocol::studio::StudioAttachmentModality::Image,
            media_type: "image/png".into(),
            filename: Some("output.png".into()),
            storage_path: "immutable-image.png".into(),
            byte_size: 4,
            content_sha256: "test-digest".into(),
            width: Some(1),
            height: Some(1),
            created_at: 1,
        };
        writer.record_attachments(vec![attachment.clone()]);
        writer.record_thread(commit.clone());
        assert!(writer.shutdown().await.is_err());
        assert_eq!(writer.pending_commit_count(), 2);
        store
            .database()
            .execute_unprepared("DROP TRIGGER fail_facts")
            .await
            .unwrap();
        writer.retry_now();
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            writer.await_durable(&thread.id, 1),
        )
        .await
        .unwrap()
        .unwrap();
        writer.record_attachments(vec![attachment.clone()]);
        writer.record_thread(commit);
        writer.flush().await.unwrap();
        pretty_assertions::assert_eq!(
            super::super::context::restore_transcript(store.database(), &thread.id)
                .await
                .unwrap(),
            transcript
        );
        pretty_assertions::assert_eq!(
            store.list_thread_attachments(&thread.id).await.unwrap(),
            vec![attachment]
        );
        let restored_billing = restore_billing(&store, &thread.id).await.unwrap();
        pretty_assertions::assert_eq!(restored_billing["saved-turn"].inferences, vec![billing]);
        let reports = super::super::submissions::list_thread_submissions(
            &store,
            &ThreadId::new(thread.id).unwrap(),
            0,
            10,
        )
        .await
        .unwrap();
        pretty_assertions::assert_eq!(
            reports.items,
            vec![pl_core::AgentSubmissionRecord::from(&submission)]
        );
        assert_eq!(reports.total, 1);
        writer.shutdown().await.unwrap();
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
            usage_json: serde_json::to_string(&pl_protocol::InferenceTokenUsage::default())
                .unwrap(),
            metadata_json: None,
            updated_at: 1,
        };
        let projected = pl_protocol::InferenceTokenUsage {
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
        cny.accounting.pricing = pl_protocol::PricingOutcome::Estimated {
            cost: RuntimeCostAmount {
                currency: "CNY".into(),
                amount: 0.5,
            },
            cache_savings: Some(RuntimeCostAmount {
                currency: "CNY".into(),
                amount: 0.2,
            }),
        };
        let mut usd = billing_record("usd", 20, 5, 4);
        usd.accounting.pricing = pl_protocol::PricingOutcome::Estimated {
            cost: RuntimeCostAmount {
                currency: "USD".into(),
                amount: 0.25,
            },
            cache_savings: Some(RuntimeCostAmount {
                currency: "USD".into(),
                amount: -0.05,
            }),
        };
        let mut unknown = billing_record("unknown", 0, 0, 0);
        unknown.accounting = Default::default();
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
                    inferences: vec![usd, unknown],
                },
            ),
        ]);
        let usage = aggregate_billing_usage(billing.values());
        let context = ThreadContextState {
            submissions: Default::default(),
            metadata: pl_core::ThreadContextMetadata::default(),
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
        assert!(runtime.usage.has_incomplete_usage);
        assert_eq!(runtime.usage.cache_hit_rate, None);
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
            has_incomplete_usage: billing.accounting.has_incomplete_usage(),
            context_tokens: billing.accounting.usage.known_total_tokens(),
            inference_id: billing.inference_id.clone(),
            agent_id: thread_id.to_string(),
            path: thread_id.to_string(),
            parent_path: None,
            role: "executor".to_string(),
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
        };
        ThreadCommit {
            agent_id: thread_id.clone(),
            persistence: PersistenceClass::Settlement,
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
        InferenceBillingRecord {
            inference_id: inference_id.to_string(),
            provider_instance_id: "deepseek-primary".to_string(),
            provider: "DeepSeek".to_string(),
            model: "deepseek-v4-flash".to_string(),
            context_window: Some(1_000_000),
            accounting: pl_protocol::InferenceAccounting {
                usage: pl_protocol::UsageReport {
                    input_tokens: Some(prompt_tokens),
                    output_tokens: Some(completion_tokens),
                    cache_read_tokens: Some(cached_prompt_tokens),
                    cache_write_tokens: None,
                    reasoning_tokens: Some(0),
                    total_tokens: Some(prompt_tokens + completion_tokens),
                },
                pricing: pl_protocol::PricingOutcome::Estimated {
                    cost: RuntimeCostAmount {
                        currency: "CNY".into(),
                        amount: 0.000_1,
                    },
                    cache_savings: None,
                },
                price_snapshot: Some(ModelPricingSnapshot {
                    currency: Some("CNY".into()),
                    input_per_mtok: Some(1.5),
                    output_per_mtok: Some(4.5),
                    cache_read_per_mtok: Some(0.05),
                    cache_write_per_mtok: None,
                }),
                request_started_at: Some(1),
            },
            prompt_generation: None,
            prompt_cache_policy: None,
            prefix_changed_reason: None,
            orchestration: Default::default(),
            timing: None,
            recorded_at: 1,
        }
    }
    async fn compatible_model_server() -> (String, tokio::task::JoinHandle<()>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            let mut chunk = [0; 4096];
            loop {
                let count = socket.read(&mut chunk).await.unwrap();
                if count == 0 {
                    panic!("incomplete request");
                }
                bytes.extend_from_slice(&chunk[..count]);
                if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                    let header = String::from_utf8_lossy(&bytes[..end]);
                    let length = header
                        .lines()
                        .find_map(|line| {
                            let (key, value) = line.split_once(':')?;
                            key.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if bytes.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            let events = [
                serde_json::json!({"choices":[{"delta":{"content":"ready"},"finish_reason":"stop"}]}),
                serde_json::json!({"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":3,"total_tokens":13,"prompt_tokens_details":{"cached_tokens":4}}}),
            ];
            let body = events
                .into_iter()
                .map(|event| format!("data: {event}\n\n"))
                .collect::<String>()
                + "data: [DONE]\n\n";
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            socket.shutdown().await.unwrap();
        });
        (format!("http://{address}/v1"), server)
    }
}
