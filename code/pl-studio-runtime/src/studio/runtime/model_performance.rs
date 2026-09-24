//! 模型性能/费用的产品投影。
//!
//! 每次推理事实以调用身份登记到全局 `calls.sqlite`（单一逻辑 writer 队列，冻结 binding、
//! 用量、价格、时延），调用库的 `(thread_id, call_id)` 唯一约束是持久 idempotency 的权威来源。
//! 性能历史、按模型汇总与会话费用都从调用库查询/聚合投影读取，不再从 studio.sqlite 的
//! `ModelPerformanceState` 历史、无界 fingerprint 集合或内部回执恢复或持久化。
//!
//! 进程内只保留固定上限的缓存：单调 revision、更新时间与最近 inference 身份窗口（256 条，
//! 且不持久化）。执行恢复（`load_cache`）只读取产品对象缓存，完全不读取调用库。

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use pl_protocol::{InferenceBillingRecord, InferenceModelObservation, ModelMatchState};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::studio::storage::calls::{
    CallRetention, PerformanceSampleRow, PerformanceSummaryRow, PurposeCostRollup,
    SessionCostRollup,
};
use crate::studio::store::object::{PersistedStudioObject, load_object, put_object};
use crate::studio::{ProductEventBus, StudioStore, unix_seconds};
use crate::{
    PureError, StudioModelPerformanceSample, StudioModelPerformanceSnapshot,
    StudioModelPerformanceSummary, StudioSessionCostSnapshot,
};

pub(in crate::studio) const MODEL_PERFORMANCE_OWNER_ID: &str = "global";
const CACHE_VERSION: u32 = 4;
const LEGACY_CACHE_VERSION: u32 = 2;
/// 产品快照返回的性能历史窗口上限；历史事实源是调用库。
const HISTORY_LIMIT: usize = 1_000;
/// 内存中保留的最近 inference 身份窗口上限。
///
/// 持久 idempotency 已由 `calls.sqlite` 的调用身份唯一约束承担；这里只用于抑制同一
/// 进程内重复投递，因此必须有界，且不参与持久化。
const RECENT_IDENTITY_LIMIT: usize = 256;

#[derive(Clone)]
pub(crate) struct ModelPerformanceOwner {
    state: Arc<Mutex<ModelPerformanceState>>,
    store: StudioStore,
    product_events: ProductEventBus,
    /// 已有刷新任务在跑；避免每次推理都重复整库聚合。
    refreshing: Arc<AtomicBool>,
    /// 已受理的最高调用库计费 ticket；刷新与快照只等待这个固定目标。
    billing_ticket: Arc<AtomicU64>,
    /// 已持久化的产品对象 revision，避免同一 revision 重复写盘。
    persisted_revision: Arc<AtomicU64>,
    /// 串行化产品对象写入，避免刷新任务与显式快照并发写同一 revision。
    persist_lock: Arc<tokio::sync::Mutex<()>>,
    /// Serializes reads and assigns revisions after the queried projection is known.
    presentation: Arc<tokio::sync::Mutex<StudioModelPerformanceSnapshot>>,
}

/// 最近 inference 身份窗口（固定上限、不持久化）。
#[derive(Debug, Clone, Default)]
struct RecentIdentities {
    fingerprints: BTreeMap<(String, String), String>,
    order: VecDeque<(String, String)>,
}

impl RecentIdentities {
    fn get(&self, root_thread_id: &str, identity: &str) -> Option<&String> {
        self.fingerprints
            .get(&(root_thread_id.to_owned(), identity.to_owned()))
    }

    fn insert(&mut self, root_thread_id: &str, identity: &str, fingerprint: String) {
        let key = (root_thread_id.to_owned(), identity.to_owned());
        if self.fingerprints.insert(key.clone(), fingerprint).is_none() {
            self.order.push_back(key);
        }
        while self.order.len() > RECENT_IDENTITY_LIMIT {
            if let Some(oldest) = self.order.pop_front() {
                self.fingerprints.remove(&oldest);
            }
        }
    }

    fn remove_root(&mut self, root_thread_id: &str) {
        self.fingerprints
            .retain(|(root, _), _| root != root_thread_id);
        self.order.retain(|(root, _)| root != root_thread_id);
    }
}

/// 模型性能领域的固定上限缓存。
///
/// 只保留版本、单调 revision、更新时间与最近身份窗口：历史、汇总、fingerprint 集合与内部
/// 回执都不在这里持久化，也不从这里恢复（历史/汇总从 `calls.sqlite` 读取，持久幂等由调用身份
/// 唯一约束承担）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::studio) struct ModelPerformanceState {
    version: u32,
    revision: u64,
    updated_at: i64,
    #[serde(skip)]
    recent: RecentIdentities,
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
            recent: RecentIdentities::default(),
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

#[derive(Clone, Copy)]
enum BillingRetention {
    Turn,
    Internal,
}

impl ModelPerformanceOwner {
    pub(in crate::studio) fn new(store: StudioStore, product_events: ProductEventBus) -> Self {
        Self {
            state: Arc::new(Mutex::new(ModelPerformanceState::default())),
            store,
            product_events,
            refreshing: Arc::new(AtomicBool::new(false)),
            billing_ticket: Arc::new(AtomicU64::new(0)),
            persisted_revision: Arc::new(AtomicU64::new(0)),
            persist_lock: Arc::new(tokio::sync::Mutex::new(())),
            presentation: Arc::new(tokio::sync::Mutex::new(
                StudioModelPerformanceSnapshot::default(),
            )),
        }
    }

    /// 恢复固定上限缓存；不读取调用库，执行恢复不依赖 calls。
    pub(crate) async fn load_cache(&self) -> Result<(), PureError> {
        let Some(restored) =
            load_object::<ModelPerformanceState>(self.store.database(), MODEL_PERFORMANCE_OWNER_ID)
                .await
                .map_err(|error| PureError::MemoryError(error.to_string()))?
        else {
            return Ok(());
        };
        if restored.version < LEGACY_CACHE_VERSION || restored.version > CACHE_VERSION {
            return Err(PureError::MemoryError(format!(
                "unsupported model performance cache version {}",
                restored.version
            )));
        }
        {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            *state = ModelPerformanceState {
                version: CACHE_VERSION,
                revision: restored.revision,
                updated_at: restored.updated_at,
                recent: RecentIdentities::default(),
            };
        }
        self.persisted_revision
            .store(restored.revision, Ordering::Release);
        self.presentation.lock().await.revision = restored.revision;
        Ok(())
    }

    /// 产品快照：历史、汇总与会话费用来自调用库查询/聚合投影。
    pub(crate) async fn snapshot(&self) -> StudioModelPerformanceSnapshot {
        let mut previous = self.presentation.lock().await;
        match self.read_snapshot().await {
            Ok(mut snapshot) => {
                snapshot.revision = previous.revision.saturating_add(1);
                *previous = snapshot.clone();
                snapshot
            }
            Err(error) => {
                tracing::warn!(error = %error, "model performance snapshot read failed");
                let mut snapshot = previous.clone();
                snapshot.revision = snapshot.revision.saturating_add(1);
                snapshot.read_failed = true;
                let calls = self.store.calls();
                snapshot.statistics_pending =
                    self.billing_ticket.load(Ordering::Acquire) > calls.durable_ticket();
                snapshot.statistics_gap = calls.statistics_gap();
                *previous = snapshot.clone();
                snapshot
            }
        }
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

    pub(crate) fn record_auxiliary_inference(
        &self,
        root_thread_id: &str,
        thread_id: &str,
        billing: &InferenceBillingRecord,
    ) -> Result<(), PureError> {
        self.record(
            root_thread_id,
            thread_id,
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
        let fingerprint = billing_fingerprint(billing)?;
        let conflict = {
            let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            match state.recent.get(root_thread_id, &billing.inference_id) {
                Some(existing) if existing == &fingerprint => return Ok(()),
                Some(_) => true,
                None => false,
            }
        };
        if conflict {
            // 同一推理身份携带不同内容，是调用方重复投递被拒，而不是已受理事实的持久化失败：
            // 这次请求从未进入调用库队列，所以只明确拒绝本次请求，绝不把它升级为写者终态。
            // 终端 `Blocked` 只留给调用库真实不可恢复的持久化冲突，避免一次应用级拒绝让整个
            // Studio 的写者健康永久失效。已受理事实与写者健康都不因这次拒绝改变。
            return Err(PureError::MemoryError(format!(
                "inference {} conflicts with the model performance owner",
                billing.inference_id
            )));
        }
        // 调用库是唯一 writer：计费观察同步受理进调用库有界队列，durability 由这里固定的
        // ticket 在刷新快照 / Thread 关闭 / Studio shutdown 时等待，绝不在受理时提前确认。
        let ticket = self
            .store
            .calls()
            .admit_billing(
                root_thread_id,
                thread_id,
                billing,
                call_retention(retention),
            )
            .map_err(|error| {
                tracing::warn!(%error, inference_id = billing.inference_id, "billing statistics dropped");
                error
            })
            .ok();
        {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            if ticket.is_some() {
                state
                    .recent
                    .insert(root_thread_id, &billing.inference_id, fingerprint);
            }
            state.revision = state.revision.saturating_add(1);
            state.updated_at = unix_seconds();
        }
        if let Some(ticket) = ticket {
            self.billing_ticket.fetch_max(ticket, Ordering::AcqRel);
        }
        self.emit_snapshot_after_flush();
        Ok(())
    }

    /// 归档/删除会话后丢弃该 root 的进程内身份窗口并发布一次领域更新。
    pub(crate) async fn remove_session(&self, root_thread_id: &str) -> Result<(), PureError> {
        {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            state.recent.remove_root(root_thread_id);
            state.revision = state.revision.saturating_add(1);
            state.updated_at = unix_seconds();
        }
        // 归档/删除的可见性由调用库 + 目录过滤保证；缓存写盘失败不回滚归档动作。
        if let Err(error) = self.persist_state().await {
            tracing::warn!(error = %error, "model performance cache persist failed");
        }
        self.emit_snapshot_after_flush();
        Ok(())
    }

    async fn read_snapshot(&self) -> Result<StudioModelPerformanceSnapshot, PureError> {
        let calls = self.store.calls();
        let settled_before_read = calls.durable_ticket();
        let admitted = self.billing_ticket.load(Ordering::Acquire);
        // Statistics are an eventual, lossy projection: reads never wait for its writer.
        // 产品对象只保留有界 revision/更新时间缓存；写盘失败不影响本次统计读取。
        if let Err(error) = self.persist_state().await {
            tracing::warn!(error = %error, "model performance cache persist failed");
        }
        let costs = calls.session_cost_rollups().await.map_err(memory_error)?;
        let samples = calls
            .recent_performance_samples(history_limit())
            .await
            .map_err(memory_error)?;
        let summaries = calls
            .performance_summary_rows()
            .await
            .map_err(memory_error)?;
        let (revision, updated_at) = {
            let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            (state.revision, state.updated_at)
        };
        // 归档/删除会话后不再对产品暴露其会话级统计；模型级汇总仍是全局用量事实。
        let archived = self.archived_roots();
        Ok(StudioModelPerformanceSnapshot {
            revision,
            updated_at,
            statistics_pending: admitted > settled_before_read,
            statistics_gap: calls.statistics_gap(),
            read_failed: false,
            session_costs: costs
                .iter()
                .filter(|rollup| !archived.contains(&rollup.root_thread_id))
                .map(session_cost_snapshot)
                .collect(),
            summaries: summaries.iter().map(summary_snapshot).collect(),
            history: samples.iter().map(history_sample).collect(),
        })
    }

    /// 把进程内有界缓存（revision/更新时间）前移写盘；同 revision 幂等，旧 revision 不回写。
    async fn persist_state(&self) -> Result<(), PureError> {
        let _guard = self.persist_lock.lock().await;
        let (revision, updated_at, snapshot) = {
            let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            (state.revision, state.updated_at, state.clone())
        };
        if revision == 0 || revision <= self.persisted_revision.load(Ordering::Acquire) {
            return Ok(());
        }
        put_object(
            self.store.database(),
            MODEL_PERFORMANCE_OWNER_ID,
            &snapshot,
            updated_at,
        )
        .await
        .map_err(|error| PureError::MemoryError(error.to_string()))?;
        self.persisted_revision.store(revision, Ordering::Release);
        Ok(())
    }

    /// 目录中已归档（产品不可见）的会话 root 集合；未登记 root 不在此集合，保持兼容。
    fn archived_roots(&self) -> std::collections::HashSet<String> {
        self.store
            .catalog()
            .entries()
            .into_iter()
            .filter(|entry| entry.archived)
            .map(|entry| entry.id)
            .collect()
    }

    /// Publish an initial projection, then follow the accepted billing ticket until settled.
    /// The writer's progress is a wakeup, not proof that a rejected mutation was committed.
    fn emit_snapshot_after_flush(&self) {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        if self.refreshing.swap(true, Ordering::AcqRel) {
            return;
        }
        let owner = self.clone();
        handle.spawn(async move {
            let calls = owner.store.calls();
            let mut durable = calls.subscribe_durable_ticket();
            let mut published = None;
            loop {
                let state = (
                    owner.revision(),
                    owner.billing_ticket.load(Ordering::Acquire),
                    *durable.borrow_and_update(),
                    calls.statistics_gap(),
                );
                if published != Some(state) {
                    let snapshot = owner.snapshot().await;
                    let _ = owner.product_events.emit_model_performance_state(snapshot);
                    published = Some(state);
                }
                let now = (
                    owner.revision(),
                    owner.billing_ticket.load(Ordering::Acquire),
                    calls.durable_ticket(),
                    calls.statistics_gap(),
                );
                if now != state {
                    continue;
                }
                if now.1 <= now.2 {
                    break;
                }
                tokio::select! {
                    change = durable.changed() => {
                        if change.is_err() { break; }
                    }
                    () = tokio::time::sleep(std::time::Duration::from_secs(1)) => {}
                }
            }
            owner.refreshing.store(false, Ordering::Release);
            let state = (
                owner.revision(),
                owner.billing_ticket.load(Ordering::Acquire),
                calls.durable_ticket(),
                calls.statistics_gap(),
            );
            if published != Some(state) {
                owner.emit_snapshot_after_flush();
            }
        });
    }

    fn revision(&self) -> u64 {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.revision
    }
}

fn memory_error(error: anyhow::Error) -> PureError {
    PureError::MemoryError(error.to_string())
}

fn history_limit() -> u32 {
    u32::try_from(HISTORY_LIMIT).unwrap_or(u32::MAX)
}

fn session_cost_snapshot(rollup: &SessionCostRollup) -> StudioSessionCostSnapshot {
    StudioSessionCostSnapshot {
        root_thread_id: rollup.root_thread_id.clone(),
        purpose_costs: rollup
            .purpose_costs
            .iter()
            .map(purpose_cost_snapshot)
            .collect(),
        estimated_costs: rollup.estimated_costs.clone(),
        has_unpriced_usage: rollup.has_unpriced_usage,
    }
}

fn purpose_cost_snapshot(rollup: &PurposeCostRollup) -> crate::StudioPurposeCostSnapshot {
    crate::StudioPurposeCostSnapshot {
        purpose: rollup.purpose.clone(),
        estimated_costs: rollup.estimated_costs.clone(),
        has_unpriced_usage: rollup.has_unpriced_usage,
    }
}

fn summary_snapshot(row: &PerformanceSummaryRow) -> StudioModelPerformanceSummary {
    let samples = row.sample_count as f64;
    StudioModelPerformanceSummary {
        provider_instance_id: row.provider_instance_id.clone(),
        provider_display_name: row.provider_display_name.clone(),
        model: row.sent_model.clone(),
        reasoning_effort: row.reasoning_effort.clone(),
        sample_count: row.sample_count,
        completion_tokens: row.completion_tokens,
        total_ttft_millis: row.total_ttft_millis,
        total_decode_millis: row.total_decode_millis,
        total_response_millis: row.total_response_millis,
        tokens_per_second: throughput(row.completion_tokens, row.total_decode_millis),
        average_ttft_millis: row.total_ttft_millis as f64 / samples,
        average_response_millis: row.total_response_millis as f64 / samples,
    }
}

fn history_sample(row: &PerformanceSampleRow) -> StudioModelPerformanceSample {
    let observation =
        row.configured_model
            .as_ref()
            .map(|configured_model| InferenceModelObservation {
                configured_model: configured_model.clone(),
                sent_model: row.sent_model.clone(),
                reported_model: row.reported_model.clone(),
            });
    StudioModelPerformanceSample {
        completed_at: row.completed_at,
        provider_instance_id: row.provider_instance_id.clone(),
        provider_display_name: row.provider_display_name.clone(),
        model: row.sent_model.clone(),
        configured_model: observation
            .as_ref()
            .map(|value| value.configured_model.clone()),
        sent_model: observation.as_ref().map(|value| value.sent_model.clone()),
        reported_model: observation
            .as_ref()
            .and_then(|value| value.reported_model.clone()),
        model_match_state: observation
            .as_ref()
            .map_or(ModelMatchState::LegacyUnknown, |value| value.match_state()),
        reasoning_effort: row.reasoning_effort.clone(),
        completion_tokens: row.completion_tokens,
        ttft_millis: row.ttft_millis,
        decode_millis: row.decode_millis,
        total_response_millis: row.response_millis,
        tokens_per_second: row
            .decode_millis
            .filter(|value| *value > 0)
            .map(|value| throughput(row.completion_tokens, value)),
    }
}

fn throughput(completion_tokens: u64, decode_millis: u64) -> f64 {
    completion_tokens as f64 * 1_000.0 / decode_millis as f64
}

fn billing_fingerprint(billing: &InferenceBillingRecord) -> Result<String, PureError> {
    let bytes = serde_json::to_vec(billing)?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

fn call_retention(retention: BillingRetention) -> CallRetention {
    match retention {
        BillingRetention::Turn => CallRetention::Turn,
        BillingRetention::Internal => CallRetention::Internal,
    }
}
