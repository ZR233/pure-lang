use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use pl_core::runtime_usage::merge_costs;
use pl_protocol::{InferenceBillingRecord, RuntimeCostAmount};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Mutex;

use crate::studio::store::object::{PersistedStudioObject, load_object};
use crate::studio::{ProductEventBus, StudioStore, unix_seconds};
use crate::{
    PureError, StudioModelPerformanceSample, StudioModelPerformanceSnapshot,
    StudioModelPerformanceSummary, StudioSessionCostSnapshot,
};

use super::super::agent_host::ThreadWriteBehindWriter;

pub(in crate::studio) const MODEL_PERFORMANCE_OWNER_ID: &str = "global";
const CACHE_VERSION: u32 = 2;
const HISTORY_LIMIT: usize = 1_000;

#[derive(Clone)]
pub(crate) struct ModelPerformanceOwner {
    state: Arc<Mutex<ModelPerformanceState>>,
    store: StudioStore,
    writer: ThreadWriteBehindWriter,
    product_events: ProductEventBus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::studio) struct ModelPerformanceState {
    version: u32,
    revision: u64,
    updated_at: i64,
    #[serde(default)]
    sessions: BTreeMap<String, SessionCostState>,
    #[serde(default)]
    history: VecDeque<PerformanceSample>,
}

/// 仅在 persistence worker 内存在的 object 编码 DTO。
#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub(in crate::studio) struct ModelPerformanceDto(ModelPerformanceState);

impl Default for ModelPerformanceState {
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

impl PersistedStudioObject for ModelPerformanceState {
    type PersistenceDto = ModelPerformanceDto;

    const OWNER_KIND: &'static str = "studio";
    const OBJECT_KIND: &'static str = "modelPerformance";
    const SCHEMA_VERSION: i64 = 1;

    fn revision(&self) -> u64 {
        self.revision
    }

    fn to_persistence_dto(&self) -> Self::PersistenceDto {
        ModelPerformanceDto(self.clone())
    }

    fn from_persistence_dto(dto: Self::PersistenceDto) -> anyhow::Result<Self> {
        Ok(dto.0)
    }
}

impl ModelPerformanceState {
    pub(in crate::studio) const fn revision(&self) -> u64 {
        self.revision
    }

    pub(in crate::studio) const fn updated_at(&self) -> i64 {
        self.updated_at
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionCostState {
    #[serde(default)]
    estimated_costs: Vec<RuntimeCostAmount>,
    #[serde(default)]
    has_unpriced_usage: bool,
    #[serde(default)]
    inference_fingerprints: BTreeMap<String, String>,
    #[serde(default)]
    internal_billing: pl_protocol::TurnBillingRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PerformanceSample {
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

#[derive(Clone, Copy)]
enum BillingRetention {
    Turn,
    Internal,
}

impl ModelPerformanceOwner {
    pub(in crate::studio) fn new(
        store: StudioStore,
        writer: ThreadWriteBehindWriter,
        product_events: ProductEventBus,
    ) -> Self {
        Self {
            state: Arc::new(Mutex::new(ModelPerformanceState::default())),
            store,
            writer,
            product_events,
        }
    }

    pub(crate) async fn load_cache(&self) -> Result<(), PureError> {
        let Some(mut restored) =
            load_object::<ModelPerformanceState>(self.store.database(), MODEL_PERFORMANCE_OWNER_ID)
                .await
                .map_err(|error| PureError::MemoryError(error.to_string()))?
        else {
            return Ok(());
        };
        if restored.version != CACHE_VERSION {
            return Err(PureError::MemoryError(format!(
                "unsupported model performance cache version {}",
                restored.version
            )));
        }
        while restored.history.len() > HISTORY_LIMIT {
            restored.history.pop_front();
        }
        *self.state.lock().unwrap_or_else(|error| error.into_inner()) = restored;
        Ok(())
    }

    pub(crate) async fn snapshot(&self) -> StudioModelPerformanceSnapshot {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        public_snapshot(&state)
    }

    pub(crate) fn record_inference(
        &self,
        root_thread_id: &str,
        thread_id: &str,
        billing: &InferenceBillingRecord,
    ) -> Result<(), PureError> {
        self.record(root_thread_id, thread_id, billing, BillingRetention::Turn)
    }

    pub(crate) async fn record_internal_inference(
        &self,
        root_thread_id: &str,
        billing: &InferenceBillingRecord,
    ) -> Result<(), PureError> {
        self.record(
            root_thread_id,
            root_thread_id,
            billing,
            BillingRetention::Internal,
        )
    }

    fn record(
        &self,
        root_thread_id: &str,
        thread_id: &str,
        billing: &InferenceBillingRecord,
        retention: BillingRetention,
    ) -> Result<(), PureError> {
        if root_thread_id.trim().is_empty() || thread_id.trim().is_empty() {
            return Err(PureError::MemoryError(
                "model performance inference is missing Thread identity".to_string(),
            ));
        }
        let identity = format!("{thread_id}:{}", billing.inference_id);
        let fingerprint = billing_fingerprint(billing)?;
        let snapshot = {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            let mut next = state.clone();
            let session = next.sessions.entry(root_thread_id.to_string()).or_default();
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
            match retention {
                BillingRetention::Turn => {}
                BillingRetention::Internal => {
                    session
                        .internal_billing
                        .append(billing.clone())
                        .map_err(PureError::MemoryError)?;
                }
            }
            session.inference_fingerprints.insert(identity, fingerprint);
            merge_costs(
                &mut session.estimated_costs,
                &billing.accounting.estimated_costs(),
            );
            session.has_unpriced_usage |= billing.accounting.has_unpriced_usage();

            if let Some(sample) = performance_sample(thread_id, billing) {
                next.history.push_back(sample);
                while next.history.len() > HISTORY_LIMIT {
                    next.history.pop_front();
                }
            }
            next.revision = next.revision.saturating_add(1);
            next.updated_at = unix_seconds();
            let snapshot = public_snapshot(&next);
            *state = next;
            self.writer.record_model_performance(state.clone());
            snapshot
        };
        self.product_events.emit_model_performance_state(snapshot);
        Ok(())
    }

    pub(crate) async fn remove_session(&self, root_thread_id: &str) -> Result<(), PureError> {
        let update = {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            if !state.sessions.contains_key(root_thread_id) {
                return Ok(());
            }
            let mut next = state.clone();
            next.sessions.remove(root_thread_id);
            next.revision = next.revision.saturating_add(1);
            next.updated_at = unix_seconds();
            let snapshot = public_snapshot(&next);
            *state = next;
            self.writer.record_model_performance(state.clone());
            Some(snapshot)
        };
        if let Some(snapshot) = update {
            self.product_events.emit_model_performance_state(snapshot);
        }
        Ok(())
    }
}

fn performance_sample(
    thread_id: &str,
    billing: &InferenceBillingRecord,
) -> Option<PerformanceSample> {
    let timing = billing
        .timing
        .filter(|timing| timing.has_throughput_sample())?;
    if billing.provider_instance_id.is_empty() || billing.model.is_empty() {
        return None;
    }
    Some(PerformanceSample {
        thread_id: thread_id.to_string(),
        inference_id: billing.inference_id.clone(),
        completed_at: billing.recorded_at,
        provider_instance_id: billing.provider_instance_id.clone(),
        provider_display_name: billing.provider.clone(),
        model: billing.model.clone(),
        completion_tokens: billing.accounting.usage.totals().completion_tokens,
        ttft_millis: timing.ttft_millis,
        decode_millis: timing.decode_millis,
        total_response_millis: timing.total_millis,
    })
}

fn public_snapshot(state: &ModelPerformanceState) -> StudioModelPerformanceSnapshot {
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

fn public_sample(sample: &PerformanceSample) -> StudioModelPerformanceSample {
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
    history: &VecDeque<PerformanceSample>,
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

fn billing_fingerprint(billing: &InferenceBillingRecord) -> Result<String, PureError> {
    let bytes = serde_json::to_vec(billing)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

#[cfg(test)]
mod tests {
    use pl_protocol::{InferenceOrchestrationMetrics, InferenceTiming};

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
    async fn priced_and_unpriced_agents_share_one_multi_currency_session() {
        let (owner, _, writer, _) = memory_owner().await;
        let mut root = billing_record("root-inference", "provider-a", "model-a", 20, 200, 1);
        root.accounting.pricing = pl_protocol::PricingOutcome::Estimated {
            cost: cost("CNY", 0.04),
            cache_savings: None,
        };
        let mut child = billing_record("child-inference", "provider-a", "model-a", 10, 100, 2);
        child.accounting.pricing = pl_protocol::PricingOutcome::Estimated {
            cost: cost("CNY", 0.10),
            cache_savings: None,
        };
        let mut usd_child =
            billing_record("usd-inference", "provider-usd", "model-usd", 10, 100, 2);
        usd_child.accounting.pricing = pl_protocol::PricingOutcome::Estimated {
            cost: cost("USD", 0.02),
            cache_savings: None,
        };
        owner
            .record_inference("root", "child-usd", &usd_child)
            .unwrap();
        let mut unmeasured =
            billing_record("unmeasured-inference", "provider-a", "model-a", 30, 300, 3);
        unmeasured.accounting.pricing = pl_protocol::PricingOutcome::Unpriced {
            reason: pl_protocol::UnpricedReason::MissingPrice,
        };
        unmeasured.timing = None;

        owner
            .record_inference("root", "root", &root)
            .expect("root billing");
        owner
            .record_inference("root", "child", &child)
            .expect("child billing");
        owner
            .record_inference("root", "child-2", &unmeasured)
            .expect("unmeasured billing");

        let snapshot = owner.snapshot().await;
        assert_eq!(snapshot.session_costs.len(), 1);
        assert_eq!(snapshot.session_costs[0].root_thread_id, "root");
        assert_eq!(
            snapshot.session_costs[0].estimated_costs,
            [cost("CNY", 0.14), cost("USD", 0.02)]
        );
        assert!(snapshot.session_costs[0].has_unpriced_usage);
        assert_eq!(snapshot.history.len(), 3);

        writer.shutdown().await.expect("writer shutdown");
    }

    #[tokio::test]
    async fn unpriced_root_does_not_hide_priced_child_session_cost() {
        let (owner, _, writer, _) = memory_owner().await;
        let mut root = billing_record("root-inference", "provider-a", "model-a", 20, 200, 1);
        root.accounting.pricing = pl_protocol::PricingOutcome::Unpriced {
            reason: pl_protocol::UnpricedReason::MissingPrice,
        };
        let mut child = billing_record("child-inference", "provider-a", "model-a", 10, 100, 2);
        child.accounting.pricing = pl_protocol::PricingOutcome::Estimated {
            cost: cost("CNY", 0.10),
            cache_savings: None,
        };

        owner
            .record_inference("root", "root", &root)
            .expect("unpriced root billing");
        owner
            .record_inference("root", "child", &child)
            .expect("priced child billing");

        let snapshot = owner.snapshot().await;
        assert_eq!(snapshot.session_costs.len(), 1);
        assert_eq!(snapshot.session_costs[0].root_thread_id, "root");
        assert_eq!(
            snapshot.session_costs[0].estimated_costs,
            [cost("CNY", 0.10)]
        );
        assert!(snapshot.session_costs[0].has_unpriced_usage);

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
            .expect("first sample");
        owner
            .record_inference(
                "root",
                "root",
                &billing_record("second", "provider-a", "shared-model", 50, 250, 2),
            )
            .expect("second sample");
        owner
            .record_inference(
                "root",
                "root",
                &billing_record("isolated", "provider-b", "shared-model", 30, 100, 3),
            )
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
            .expect("record inference");
        assert_eq!(owner.snapshot().await.revision, 1);
        let event = receiver.recv().await.expect("performance product event");
        assert!(matches!(
            event.kind,
            StudioProductEventKind::ModelPerformanceStateChanged(snapshot)
                if snapshot.revision == 1
        ));

        // An internal title request has no Turn transcript, so its full receipt is retained here.
        use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut reader = tokio::io::BufReader::new(&mut socket);
            let mut content_length = 0;
            loop {
                let mut header = String::new();
                assert!(reader.read_line(&mut header).await.unwrap() > 0);
                if header == "\r\n" {
                    break;
                }
                if let Some((name, value)) = header.split_once(':')
                    && name.eq_ignore_ascii_case("content-length")
                {
                    content_length = value.trim().parse::<usize>().unwrap();
                }
            }
            reader
                .read_exact(&mut vec![0; content_length])
                .await
                .unwrap();
            drop(reader);
            let body = concat!(
                "data: {\"choices\":[{\"delta\":{\"content\":\"Session title\"},\"finish_reason\":\"stop\"}]}\n\n",
                "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":10,\"total_tokens\":110,\"prompt_tokens_details\":{\"cached_tokens\":20}}}\n\n",
                "data: [DONE]\n\n"
            );
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        });
        let model = pl_model::runtime::ModelRuntime::new(
            pl_model::provider::ProviderEndpoint::compatible(
                "title",
                format!("http://{address}/v1"),
            ),
            pl_model::model::ModelInfo::compatible("title-model"),
        )
        .unwrap()
        .with_pricing_mode(pl_protocol::PricingMode::Disabled);
        let title = model
            .complete(
                pl_model::completion::CompletionRequest::builder()
                    .instructions("Name this session")
                    .build(),
                Default::default(),
            )
            .await
            .unwrap();
        server.await.unwrap();
        assert_eq!(title.content.as_deref(), Some("Session title"));
        let mut internal = billing_record("title", "provider-a", "title-model", 10, 100, 8);
        internal.accounting = title.accounting;
        owner
            .record_internal_inference("root", &internal)
            .await
            .unwrap();
        owner
            .record_internal_inference("root", &internal)
            .await
            .unwrap();

        writer.flush().await.expect("flush observed state");
        assert!(
            load_object::<ModelPerformanceState>(store.database(), MODEL_PERFORMANCE_OWNER_ID,)
                .await
                .unwrap()
                .is_some()
        );

        let product_events = ProductEventBus::new(store.clone(), writer.clone());
        let restored = ModelPerformanceOwner::new(store.clone(), writer.clone(), product_events);
        restored.load_cache().await.expect("restore cache");
        assert_eq!(restored.snapshot().await, owner.snapshot().await);
        let restored_receipts = restored.state.lock().unwrap().sessions["root"]
            .internal_billing
            .clone();
        assert_eq!(restored_receipts.inferences, vec![internal]);

        let empty_store = StudioStore::open_memory().await.expect("empty store");
        let empty_writer = ThreadWriteBehindWriter::new(empty_store.clone());
        let product_events = ProductEventBus::new(empty_store.clone(), empty_writer.clone());
        let empty = ModelPerformanceOwner::new(empty_store, empty_writer.clone(), product_events);
        empty.load_cache().await.expect("load empty cache");
        assert_eq!(
            empty.snapshot().await,
            StudioModelPerformanceSnapshot::default()
        );

        writer.shutdown().await.expect("writer shutdown");
        empty_writer
            .shutdown()
            .await
            .expect("empty writer shutdown");
    }

    #[tokio::test]
    async fn inference_identity_is_idempotent_and_rejects_conflicts() {
        let (owner, _, _, _) = memory_owner().await;
        let billing = billing_record("same", "provider-a", "model-a", 10, 100, 1);
        owner
            .record_inference("root", "child", &billing)
            .expect("first record");
        owner
            .record_inference("root", "child", &billing)
            .expect("identical retry");
        assert_eq!(owner.snapshot().await.revision, 1);

        let mut conflict = billing;
        conflict.accounting.usage.output_tokens = Some(11);
        conflict.accounting.usage.total_tokens = Some(31);
        assert!(owner.record_inference("root", "child", &conflict).is_err());
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
        InferenceBillingRecord {
            inference_id: inference_id.to_string(),
            provider_instance_id: provider_instance_id.to_string(),
            provider: format!("{provider_instance_id} display"),
            model: model.to_string(),
            context_window: Some(128_000),
            accounting: pl_protocol::InferenceAccounting {
                usage: pl_protocol::UsageReport {
                    input_tokens: Some(20),
                    cache_read_tokens: Some(0),
                    cache_write_tokens: Some(0),
                    output_tokens: Some(completion_tokens),
                    reasoning_tokens: Some(completion_tokens / 2),
                    total_tokens: Some(20 + completion_tokens),
                },
                pricing: pl_protocol::PricingOutcome::Disabled,
                price_snapshot: None,
                request_started_at: Some(recorded_at),
            },
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
