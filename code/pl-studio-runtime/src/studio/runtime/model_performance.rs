use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use pl_protocol::{InferenceBillingRecord, RuntimeCostAmount};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

use crate::studio::{ProductEventBus, StudioStore, unix_seconds};
use crate::{
    PureError, StudioModelPerformanceSample, StudioModelPerformanceSnapshot,
    StudioModelPerformanceSummary, StudioSessionCostSnapshot,
};

use super::super::agent_host::ThreadWriteBehindWriter;

const CACHE_KEY: &str = "observed:modelPerformance:v1";
const CACHE_VERSION: u32 = 1;
const HISTORY_LIMIT: usize = 1_000;

#[derive(Clone)]
pub(crate) struct ModelPerformanceOwner {
    state: Arc<Mutex<PersistedModelPerformance>>,
    store: StudioStore,
    writer: ThreadWriteBehindWriter,
    product_events: ProductEventBus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedModelPerformance {
    version: u32,
    revision: u64,
    updated_at: i64,
    #[serde(default)]
    sessions: BTreeMap<String, PersistedSessionCost>,
    #[serde(default)]
    history: VecDeque<PersistedPerformanceSample>,
}

impl Default for PersistedModelPerformance {
    fn default() -> Self {
        Self {
            version: CACHE_VERSION,
            revision: 0,
            updated_at: 0,
            sessions: BTreeMap::new(),
            history: VecDeque::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedSessionCost {
    #[serde(default)]
    estimated_costs: Vec<RuntimeCostAmount>,
    #[serde(default)]
    has_unpriced_usage: bool,
    #[serde(default)]
    inference_fingerprints: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedPerformanceSample {
    thread_id: String,
    inference_id: String,
    completed_at: i64,
    provider_instance_id: String,
    provider_display_name: String,
    model: String,
    completion_tokens: u64,
    ttft_millis: u64,
    decode_millis: u64,
    total_response_millis: u64,
}

impl ModelPerformanceOwner {
    pub(in crate::studio) fn new(
        store: StudioStore,
        writer: ThreadWriteBehindWriter,
        product_events: ProductEventBus,
    ) -> Self {
        Self {
            state: Arc::new(Mutex::new(PersistedModelPerformance::default())),
            store,
            writer,
            product_events,
        }
    }

    pub(crate) async fn load_cache(&self) -> Result<(), PureError> {
        let Some(value) = self
            .store
            .load_setting(CACHE_KEY)
            .await
            .map_err(|error| PureError::MemoryError(error.to_string()))?
        else {
            return Ok(());
        };
        let mut restored: PersistedModelPerformance = serde_json::from_str(&value)?;
        if restored.version != CACHE_VERSION {
            return Err(PureError::MemoryError(format!(
                "unsupported model performance cache version {}",
                restored.version
            )));
        }
        while restored.history.len() > HISTORY_LIMIT {
            restored.history.pop_front();
        }
        *self.state.lock().await = restored;
        Ok(())
    }

    pub(crate) async fn snapshot(&self) -> StudioModelPerformanceSnapshot {
        let state = self.state.lock().await;
        public_snapshot(&state)
    }

    pub(crate) async fn record_inference(
        &self,
        root_thread_id: &str,
        thread_id: &str,
        billing: &InferenceBillingRecord,
    ) -> Result<(), PureError> {
        if root_thread_id.trim().is_empty() || thread_id.trim().is_empty() {
            return Err(PureError::MemoryError(
                "model performance inference is missing Thread identity".to_string(),
            ));
        }
        let identity = format!("{thread_id}:{}", billing.inference_id);
        let fingerprint = billing_fingerprint(billing)?;
        let (snapshot, revision, value) = {
            let mut state = self.state.lock().await;
            let session = state
                .sessions
                .entry(root_thread_id.to_string())
                .or_default();
            if let Some(existing) = session.inference_fingerprints.get(&identity) {
                if existing == &fingerprint {
                    return Ok(());
                }
                let reason = format!(
                    "inference {} conflicts with the model performance owner",
                    billing.inference_id
                );
                self.writer.block(&reason);
                return Err(PureError::MemoryError(reason));
            }
            session.inference_fingerprints.insert(identity, fingerprint);
            merge_costs(&mut session.estimated_costs, &billing.estimated_costs);
            session.has_unpriced_usage |= billing.has_unpriced_usage;

            if let Some(sample) = performance_sample(thread_id, billing) {
                state.history.push_back(sample);
                while state.history.len() > HISTORY_LIMIT {
                    state.history.pop_front();
                }
            }
            state.revision = state.revision.saturating_add(1);
            state.updated_at = unix_seconds();
            let snapshot = public_snapshot(&state);
            let value = serde_json::to_string(&*state)?;
            (snapshot, state.revision, value)
        };
        self.product_events.emit_model_performance_state(snapshot);
        self.writer
            .enqueue_observed_state(CACHE_KEY, revision, value)
            .await
    }

    pub(crate) async fn remove_session(&self, root_thread_id: &str) -> Result<(), PureError> {
        let update = {
            let mut state = self.state.lock().await;
            if state.sessions.remove(root_thread_id).is_none() {
                return Ok(());
            }
            state.revision = state.revision.saturating_add(1);
            state.updated_at = unix_seconds();
            Some((
                public_snapshot(&state),
                state.revision,
                serde_json::to_string(&*state)?,
            ))
        };
        if let Some((snapshot, revision, value)) = update {
            self.product_events.emit_model_performance_state(snapshot);
            self.writer
                .enqueue_observed_state(CACHE_KEY, revision, value)
                .await?;
        }
        Ok(())
    }
}

fn performance_sample(
    thread_id: &str,
    billing: &InferenceBillingRecord,
) -> Option<PersistedPerformanceSample> {
    let timing = billing
        .timing
        .filter(|timing| timing.has_throughput_sample())?;
    if billing.provider_instance_id.is_empty() || billing.model.is_empty() {
        return None;
    }
    Some(PersistedPerformanceSample {
        thread_id: thread_id.to_string(),
        inference_id: billing.inference_id.clone(),
        completed_at: billing.recorded_at,
        provider_instance_id: billing.provider_instance_id.clone(),
        provider_display_name: billing.provider.clone(),
        model: billing.model.clone(),
        completion_tokens: billing.normalized_usage.completion_tokens,
        ttft_millis: timing.ttft_millis,
        decode_millis: timing.decode_millis,
        total_response_millis: timing.total_millis,
    })
}

fn public_snapshot(state: &PersistedModelPerformance) -> StudioModelPerformanceSnapshot {
    let session_costs = state
        .sessions
        .iter()
        .map(|(root_thread_id, session)| StudioSessionCostSnapshot {
            root_thread_id: root_thread_id.clone(),
            estimated_costs: session.estimated_costs.clone(),
            has_unpriced_usage: session.has_unpriced_usage,
        })
        .collect();
    let history = state.history.iter().rev().map(public_sample).collect();
    StudioModelPerformanceSnapshot {
        revision: state.revision,
        updated_at: state.updated_at,
        session_costs,
        summaries: performance_summaries(&state.history),
        history,
    }
}

fn public_sample(sample: &PersistedPerformanceSample) -> StudioModelPerformanceSample {
    StudioModelPerformanceSample {
        completed_at: sample.completed_at,
        provider_instance_id: sample.provider_instance_id.clone(),
        provider_display_name: sample.provider_display_name.clone(),
        model: sample.model.clone(),
        completion_tokens: sample.completion_tokens,
        ttft_millis: sample.ttft_millis,
        decode_millis: sample.decode_millis,
        total_response_millis: sample.total_response_millis,
        tokens_per_second: throughput(sample.completion_tokens, sample.decode_millis),
    }
}

#[derive(Default)]
struct SummaryAccumulator {
    provider_display_name: String,
    sample_count: u64,
    completion_tokens: u64,
    total_ttft_millis: u64,
    total_decode_millis: u64,
    total_response_millis: u64,
}

fn performance_summaries(
    history: &VecDeque<PersistedPerformanceSample>,
) -> Vec<StudioModelPerformanceSummary> {
    let mut groups = BTreeMap::<(String, String), SummaryAccumulator>::new();
    for sample in history {
        let aggregate = groups
            .entry((sample.provider_instance_id.clone(), sample.model.clone()))
            .or_default();
        aggregate
            .provider_display_name
            .clone_from(&sample.provider_display_name);
        aggregate.sample_count = aggregate.sample_count.saturating_add(1);
        aggregate.completion_tokens = aggregate
            .completion_tokens
            .saturating_add(sample.completion_tokens);
        aggregate.total_ttft_millis = aggregate
            .total_ttft_millis
            .saturating_add(sample.ttft_millis);
        aggregate.total_decode_millis = aggregate
            .total_decode_millis
            .saturating_add(sample.decode_millis);
        aggregate.total_response_millis = aggregate
            .total_response_millis
            .saturating_add(sample.total_response_millis);
    }
    groups
        .into_iter()
        .map(|((provider_instance_id, model), aggregate)| {
            let sample_count = aggregate.sample_count as f64;
            StudioModelPerformanceSummary {
                provider_instance_id,
                provider_display_name: aggregate.provider_display_name,
                model,
                sample_count: aggregate.sample_count,
                completion_tokens: aggregate.completion_tokens,
                total_ttft_millis: aggregate.total_ttft_millis,
                total_decode_millis: aggregate.total_decode_millis,
                total_response_millis: aggregate.total_response_millis,
                tokens_per_second: throughput(
                    aggregate.completion_tokens,
                    aggregate.total_decode_millis,
                ),
                average_ttft_millis: aggregate.total_ttft_millis as f64 / sample_count,
                average_response_millis: aggregate.total_response_millis as f64 / sample_count,
            }
        })
        .collect()
}

fn throughput(completion_tokens: u64, decode_millis: u64) -> f64 {
    completion_tokens as f64 * 1_000.0 / decode_millis as f64
}

fn merge_costs(target: &mut Vec<RuntimeCostAmount>, incoming: &[RuntimeCostAmount]) {
    for cost in incoming {
        if let Some(existing) = target
            .iter_mut()
            .find(|existing| existing.currency == cost.currency)
        {
            existing.amount += cost.amount;
        } else {
            target.push(cost.clone());
        }
    }
    target.sort_by(|left, right| left.currency.cmp(&right.currency));
}

fn billing_fingerprint(billing: &InferenceBillingRecord) -> Result<String, PureError> {
    let bytes = serde_json::to_vec(billing)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

#[cfg(test)]
mod tests {
    use pl_protocol::{
        InferenceOrchestrationMetrics, InferenceTiming, InferenceTokenUsage, ModelPricingSnapshot,
    };

    use super::*;
    use crate::StudioProductEventKind;

    async fn memory_owner() -> (
        ModelPerformanceOwner,
        StudioStore,
        ThreadWriteBehindWriter,
        tokio::sync::broadcast::Receiver<crate::StudioProductEventEnvelope>,
    ) {
        let store = StudioStore::open_memory().await.expect("memory store");
        let writer = ThreadWriteBehindWriter::new(store.clone());
        let product_events = ProductEventBus::new(store.clone(), writer.clone());
        let receiver = product_events.subscribe();
        (
            ModelPerformanceOwner::new(store.clone(), writer.clone(), product_events),
            store,
            writer,
            receiver,
        )
    }

    #[tokio::test]
    async fn root_and_child_costs_share_one_multi_currency_session() {
        let (owner, _, writer, _) = memory_owner().await;
        let mut root = billing_record("root-inference", "provider-a", "model-a", 20, 200, 1);
        root.estimated_costs = vec![cost("CNY", 0.04)];
        root.has_unpriced_usage = true;
        let mut child = billing_record("child-inference", "provider-a", "model-a", 10, 100, 2);
        child.estimated_costs = vec![cost("CNY", 0.10), cost("USD", 0.02)];
        let mut unmeasured =
            billing_record("unmeasured-inference", "provider-a", "model-a", 30, 300, 3);
        unmeasured.timing = None;

        owner
            .record_inference("root", "root", &root)
            .await
            .expect("root billing");
        owner
            .record_inference("root", "child", &child)
            .await
            .expect("child billing");
        owner
            .record_inference("root", "child-2", &unmeasured)
            .await
            .expect("unmeasured billing");

        let snapshot = owner.snapshot().await;
        assert_eq!(snapshot.session_costs.len(), 1);
        assert_eq!(snapshot.session_costs[0].root_thread_id, "root");
        assert_eq!(
            snapshot.session_costs[0].estimated_costs,
            [cost("CNY", 0.14), cost("USD", 0.02)]
        );
        assert!(snapshot.session_costs[0].has_unpriced_usage);
        assert_eq!(snapshot.history.len(), 2);

        writer.shutdown().await.expect("writer shutdown");
    }

    #[tokio::test]
    async fn summaries_isolate_provider_instances_and_use_weighted_throughput() {
        let (owner, _, writer, _) = memory_owner().await;
        owner
            .record_inference(
                "root",
                "root",
                &billing_record("first", "provider-a", "shared-model", 100, 1_000, 1),
            )
            .await
            .expect("first sample");
        owner
            .record_inference(
                "root",
                "root",
                &billing_record("second", "provider-a", "shared-model", 50, 250, 2),
            )
            .await
            .expect("second sample");
        owner
            .record_inference(
                "root",
                "root",
                &billing_record("isolated", "provider-b", "shared-model", 30, 100, 3),
            )
            .await
            .expect("isolated sample");

        let snapshot = owner.snapshot().await;
        assert_eq!(snapshot.summaries.len(), 2);
        let aggregate = snapshot
            .summaries
            .iter()
            .find(|summary| summary.provider_instance_id == "provider-a")
            .expect("provider-a summary");
        assert_eq!(aggregate.sample_count, 2);
        assert_eq!(aggregate.completion_tokens, 150);
        assert_eq!(aggregate.total_decode_millis, 1_250);
        assert_eq!(aggregate.tokens_per_second, 120.0);
        assert_eq!(aggregate.average_ttft_millis, 10.0);

        writer.shutdown().await.expect("writer shutdown");
    }

    #[tokio::test]
    async fn history_keeps_only_the_latest_thousand_samples() {
        let (owner, _, writer, _) = memory_owner().await;
        for index in 0..=HISTORY_LIMIT {
            owner
                .record_inference(
                    "root",
                    "root",
                    &billing_record(
                        &format!("inference-{index}"),
                        "provider-a",
                        "model-a",
                        1,
                        10,
                        index as i64,
                    ),
                )
                .await
                .expect("history sample");
        }

        let snapshot = owner.snapshot().await;
        assert_eq!(snapshot.history.len(), HISTORY_LIMIT);
        assert_eq!(snapshot.history.first().unwrap().completed_at, 1_000);
        assert_eq!(snapshot.history.last().unwrap().completed_at, 1);

        writer.shutdown().await.expect("writer shutdown");
    }

    #[tokio::test]
    async fn memory_event_precedes_flush_and_cache_restores_without_backfill() {
        let (owner, store, writer, mut receiver) = memory_owner().await;
        let billing = billing_record("inference", "provider-a", "model-a", 15, 100, 7);

        owner
            .record_inference("root", "child", &billing)
            .await
            .expect("record inference");
        assert_eq!(owner.snapshot().await.revision, 1);
        let event = receiver.recv().await.expect("performance product event");
        assert!(matches!(
            event.kind,
            StudioProductEventKind::ModelPerformanceStateChanged(snapshot)
                if snapshot.revision == 1
        ));

        writer.flush().await.expect("flush observed state");
        assert!(store.load_setting(CACHE_KEY).await.unwrap().is_some());

        let product_events = ProductEventBus::new(store.clone(), writer.clone());
        let restored = ModelPerformanceOwner::new(store.clone(), writer.clone(), product_events);
        restored.load_cache().await.expect("restore cache");
        assert_eq!(restored.snapshot().await, owner.snapshot().await);

        store
            .save_setting(
                CACHE_KEY,
                &serde_json::to_string(&PersistedModelPerformance::default()).unwrap(),
            )
            .await
            .expect("replace with empty cache");
        let product_events = ProductEventBus::new(store.clone(), writer.clone());
        let empty = ModelPerformanceOwner::new(store, writer.clone(), product_events);
        empty.load_cache().await.expect("load empty cache");
        assert_eq!(
            empty.snapshot().await,
            StudioModelPerformanceSnapshot::default()
        );

        writer.shutdown().await.expect("writer shutdown");
    }

    #[tokio::test]
    async fn inference_identity_is_idempotent_and_rejects_conflicts() {
        let (owner, _, _, _) = memory_owner().await;
        let billing = billing_record("same", "provider-a", "model-a", 10, 100, 1);
        owner
            .record_inference("root", "child", &billing)
            .await
            .expect("first record");
        owner
            .record_inference("root", "child", &billing)
            .await
            .expect("identical retry");
        assert_eq!(owner.snapshot().await.revision, 1);

        let mut conflict = billing;
        conflict.normalized_usage.completion_tokens = 11;
        assert!(
            owner
                .record_inference("root", "child", &conflict)
                .await
                .is_err()
        );
        let snapshot = owner.snapshot().await;
        assert_eq!(snapshot.revision, 1);
        assert_eq!(snapshot.history.len(), 1);
    }

    fn billing_record(
        inference_id: &str,
        provider_instance_id: &str,
        model: &str,
        completion_tokens: u64,
        decode_millis: u64,
        recorded_at: i64,
    ) -> InferenceBillingRecord {
        let usage = InferenceTokenUsage {
            prompt_tokens: 20,
            cached_prompt_tokens: 0,
            cache_write_tokens: 0,
            completion_tokens,
            reasoning_tokens: completion_tokens / 2,
            total_tokens: 20 + completion_tokens,
        };
        InferenceBillingRecord {
            inference_id: inference_id.to_string(),
            provider_instance_id: provider_instance_id.to_string(),
            provider: format!("{provider_instance_id} display"),
            model: model.to_string(),
            context_window: Some(128_000),
            reported_usage: usage.clone(),
            normalized_usage: usage,
            pricing: ModelPricingSnapshot::default(),
            estimated_costs: Vec::new(),
            estimated_cache_savings: Vec::new(),
            has_unpriced_usage: false,
            prompt_generation: None,
            prompt_cache_policy: None,
            prefix_changed_reason: None,
            orchestration: InferenceOrchestrationMetrics::default(),
            timing: Some(InferenceTiming {
                ttft_millis: 10,
                decode_millis,
                total_millis: decode_millis + 10,
            }),
            recorded_at,
        }
    }

    fn cost(currency: &str, amount: f64) -> RuntimeCostAmount {
        RuntimeCostAmount {
            currency: currency.to_string(),
            amount,
        }
    }
}
