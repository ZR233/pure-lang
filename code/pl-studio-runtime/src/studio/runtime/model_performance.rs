//! 模型性能/费用的产品投影。
//!
//! 会话 history.sqlite 的原子提交拥有累计用量与费用；calls.sqlite 只保存带 revision 的
//! 绝对统计投影和最多 3000 条性能摘要。调用正文位于有保留期限的 JSONL，日志淘汰不改变
//! 会话统计。内存仅保留固定上限的最近身份窗口，执行恢复不依赖调用日志。
//!
//! 投影经 typed product topic 对外发布，进程内只保留固定上限的缓存：单调 revision、更新时间
//! 与最近 inference 身份窗口（256 条，且不持久化）。执行恢复（`load_cache`）只读取产品对象
//! 缓存，完全不读取调用库。
//!
//! 展示投影只由明确的前进点建立：startup 初始化（`initialize_projection`）、计费
//! durability/flags 变化（`emit_snapshot_after_flush`）与归档/恢复命令。全局性能与逐 root
//! 会话费用由同一计费 owner 分别发布（作用域 revision 按 root 独立单调，含清除与淘汰）；
//! 查询（snapshot/snapshot_and_costs/session_costs_snapshot）只读已发布值：不写库、
//! 不推进 revision、不扫描补建作用域、不发事件。

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use pl_protocol::{InferenceBillingRecord, InferenceModelObservation, ModelMatchState};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::studio::storage::calls::{
    CallRetention, CallStatus, PerformanceSampleRow, PerformanceSummaryRow, PurposeCostRollup,
    SessionCostRollup,
};
use crate::studio::store::object::{PersistedStudioObject, load_object, put_object};
use crate::studio::{ProductEventBus, StudioStore, unix_seconds};
use crate::{
    PureError, StudioModelPerformanceSample, StudioModelPerformanceSnapshot,
    StudioModelPerformanceSummary, StudioSessionCostSnapshot, StudioSessionCostsState,
};

pub(in crate::studio) const MODEL_PERFORMANCE_OWNER_ID: &str = "global";
const CACHE_VERSION: u32 = 4;
const LEGACY_CACHE_VERSION: u32 = 2;
/// 产品快照返回的性能历史窗口上限；历史事实源是调用库。
const HISTORY_LIMIT: usize = crate::studio::storage::calls::PERFORMANCE_SAMPLE_LIMIT;
/// 内存中保留的最近 inference 身份窗口上限。
///
/// 持久 idempotency 已由 `calls.sqlite` 的调用身份唯一约束承担；这里只用于抑制同一
/// 进程内重复投递，因此必须有界，且不参与持久化。
const RECENT_IDENTITY_LIMIT: usize = 256;
/// 作用域费用缓存的条目上限；超限时优先淘汰已清除（`cost == None`）的最旧条目。
const SCOPED_COST_LIMIT: usize = 256;

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
    /// 串行化投影建立：调用库读取、提交与发布在同 owner 内按序完成。
    project_lock: Arc<tokio::sync::Mutex<()>>,
    /// 已发布的展示投影：全局性能与逐 root 费用同一次发布原子可见。
    published: Arc<tokio::sync::Mutex<PublishedProjection>>,
}

/// 当前已发布的展示投影；查询路径只 clone，不推进。
struct PublishedProjection {
    global: StudioModelPerformanceSnapshot,
    /// 费用发布水位独立于全局统计；跨缓存淘汰仍给每个 root 单调的展示版本。
    scoped_revision: u64,
    scoped_updated_at: i64,
    /// 每个 root 会话的作用域费用发布状态；revision 按 root 独立单调，互不覆盖。
    scoped: BTreeMap<String, ScopedCostEntry>,
}

/// 一次调用库投影读取得到的完整事实：全局性能与各 root 会话费用。
struct PerformanceProjection {
    global: StudioModelPerformanceSnapshot,
    session_costs: Vec<StudioSessionCostSnapshot>,
}

/// 单个 root 的作用域费用发布状态。
#[derive(Debug, Clone)]
struct ScopedCostEntry {
    revision: u64,
    updated_at: i64,
    statistics_pending: bool,
    statistics_gap: bool,
    read_failed: bool,
    cost: Option<StudioSessionCostSnapshot>,
}

impl ScopedCostEntry {
    fn differs(
        &self,
        pending: bool,
        gap: bool,
        read_failed: bool,
        cost: &Option<StudioSessionCostSnapshot>,
    ) -> bool {
        self.statistics_pending != pending
            || self.statistics_gap != gap
            || self.read_failed != read_failed
            || &self.cost != cost
    }

    fn state(&self, root_thread_id: &str) -> StudioSessionCostsState {
        StudioSessionCostsState {
            root_thread_id: root_thread_id.to_owned(),
            revision: self.revision,
            updated_at: self.updated_at,
            statistics_pending: self.statistics_pending,
            statistics_gap: self.statistics_gap,
            read_failed: self.read_failed,
            cost: self.cost.clone(),
        }
    }
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
            project_lock: Arc::new(tokio::sync::Mutex::new(())),
            published: Arc::new(tokio::sync::Mutex::new(PublishedProjection {
                global: StudioModelPerformanceSnapshot::default(),
                scoped_revision: 0,
                scoped_updated_at: 0,
                scoped: BTreeMap::new(),
            })),
        }
    }

    /// 恢复固定上限缓存；不读取调用库，执行恢复不依赖 calls。
    pub(crate) async fn load_cache(&self) -> anyhow::Result<()> {
        let calls = self.store.calls();
        let (costs, samples, summaries) = tokio::join!(
            calls.session_cost_rollups(),
            calls.recent_performance_samples(history_limit()),
            calls.performance_summary_rows()
        );
        costs?;
        samples?;
        summaries?;
        let Some(restored) =
            load_object::<ModelPerformanceState>(self.store.database(), MODEL_PERFORMANCE_OWNER_ID)
                .await?
        else {
            return Ok(());
        };
        if restored.version < LEGACY_CACHE_VERSION || restored.version > CACHE_VERSION {
            return Err(crate::studio::startup::data_error(anyhow::anyhow!(
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
        self.published.lock().await.global.revision = restored.revision;
        Ok(())
    }

    /// 产品快照：仅返回 owner 已发布的当前展示投影。
    ///
    /// 纯读（design/18 §18.2.1/18.4）：不写库、不推进 revision、不扫描补建作用域、
    /// 不发事件。全局性能与逐 root 费用来自同一次发布，读取端看到一致的成对值。
    pub(crate) async fn snapshot_and_costs(
        &self,
    ) -> (StudioModelPerformanceSnapshot, Vec<StudioSessionCostsState>) {
        let published = self.published.lock().await;
        (
            published.global.clone(),
            published
                .scoped
                .iter()
                .map(|(root, entry)| entry.state(root))
                .collect(),
        )
    }

    /// 全局模型性能快照（已发布值；不含按 root 会话费用）。
    pub(crate) async fn snapshot(&self) -> StudioModelPerformanceSnapshot {
        self.published.lock().await.global.clone()
    }

    /// 单个 root 会话的作用域费用基线；纯读已发布缓存。
    ///
    /// 未缓存 root 返回当前费用水位的空基线，不落缓存、不扫描调用库；即使清除条目
    /// 已被淘汰，也可覆盖客户端更旧的费用。flags 来自同一次已发布投影。
    pub(crate) async fn session_costs_snapshot(
        &self,
        root_thread_id: &str,
    ) -> StudioSessionCostsState {
        let published = self.published.lock().await;
        if let Some(entry) = published.scoped.get(root_thread_id) {
            return entry.state(root_thread_id);
        }
        ScopedCostEntry {
            revision: published.scoped_revision,
            updated_at: published.scoped_updated_at,
            statistics_pending: published.global.statistics_pending,
            statistics_gap: published.global.statistics_gap,
            read_failed: published.global.read_failed,
            cost: None,
        }
        .state(root_thread_id)
    }

    /// 在同一次 `published` 锁内原子采用一次投影的全局与作用域费用。
    ///
    /// 作用域真值未变化的 root 不提升 revision、不发事件；已发布过费用但不在最新投影
    /// 中的 root 被显式清除（`cost == None`），归档/删除对该作用域消费者可见。返回
    /// 已采用的全局快照与需要发布的作用域事实；typed topic 发布由调用方在锁外完成。
    async fn commit_projection(
        &self,
        mut global: StudioModelPerformanceSnapshot,
        fresh: Vec<StudioSessionCostSnapshot>,
        flags: (bool, bool),
    ) -> (StudioModelPerformanceSnapshot, Vec<StudioSessionCostsState>) {
        let mut events = Vec::new();
        {
            let mut published = self.published.lock().await;
            let flags_changed = (
                published.global.statistics_pending,
                published.global.statistics_gap,
                published.global.read_failed,
            ) != (flags.0, flags.1, false);
            let scoped_revision = published.scoped_revision.saturating_add(1);
            global.revision = published.global.revision.saturating_add(1);
            published.global = global.clone();
            let scoped = &mut published.scoped;
            let mut seen = std::collections::BTreeSet::new();
            for cost in fresh {
                seen.insert(cost.root_thread_id.clone());
                let root = cost.root_thread_id.clone();
                let entry = scoped
                    .entry(root.clone())
                    .or_insert_with(|| ScopedCostEntry {
                        revision: 0,
                        updated_at: unix_seconds(),
                        statistics_pending: flags.0,
                        statistics_gap: flags.1,
                        read_failed: false,
                        cost: None,
                    });
                if entry.differs(flags.0, flags.1, false, &Some(cost.clone())) {
                    entry.revision = scoped_revision;
                    entry.updated_at = unix_seconds();
                    entry.statistics_pending = flags.0;
                    entry.statistics_gap = flags.1;
                    entry.read_failed = false;
                    entry.cost = Some(cost);
                    events.push(entry.state(&root));
                }
            }
            for (root, entry) in scoped.iter_mut() {
                if !seen.contains(root) && entry.differs(flags.0, flags.1, false, &None) {
                    entry.cost = None;
                    entry.revision = scoped_revision;
                    entry.updated_at = unix_seconds();
                    entry.statistics_pending = flags.0;
                    entry.statistics_gap = flags.1;
                    entry.read_failed = false;
                    events.push(entry.state(root));
                }
            }
            evict_cleared(scoped);
            if flags_changed || !events.is_empty() {
                published.scoped_revision = scoped_revision;
                published.scoped_updated_at = unix_seconds();
            }
        }
        (global, events)
    }

    /// 读失败时在同一次 `published` 锁内原子标记全局与各 root：保留最后可用数据，
    /// 只显式标记失败与 flags；返回需要发布的作用域事实供锁外发布。
    async fn commit_read_failure(
        &self,
        pending: bool,
        gap: bool,
    ) -> (StudioModelPerformanceSnapshot, Vec<StudioSessionCostsState>) {
        let mut events = Vec::new();
        let global = {
            let mut published = self.published.lock().await;
            let flags_changed = (
                published.global.statistics_pending,
                published.global.statistics_gap,
                published.global.read_failed,
            ) != (pending, gap, true);
            let scoped_revision = published.scoped_revision.saturating_add(1);
            published.global.revision = published.global.revision.saturating_add(1);
            published.global.statistics_pending = pending;
            published.global.statistics_gap = gap;
            published.global.read_failed = true;
            for (root, entry) in published.scoped.iter_mut() {
                if entry.differs(pending, gap, true, &entry.cost.clone()) {
                    entry.revision = scoped_revision;
                    entry.updated_at = unix_seconds();
                    entry.statistics_pending = pending;
                    entry.statistics_gap = gap;
                    entry.read_failed = true;
                    events.push(entry.state(root));
                }
            }
            if flags_changed || !events.is_empty() {
                published.scoped_revision = scoped_revision;
                published.scoped_updated_at = unix_seconds();
            }
            published.global.clone()
        };
        (global, events)
    }

    fn statistics_flags(&self) -> (bool, bool) {
        let calls = self.store.calls();
        (
            self.billing_ticket.load(Ordering::Acquire) > calls.durable_ticket(),
            calls.statistics_gap(),
        )
    }

    pub(crate) fn record_inference(
        &self,
        root_thread_id: &str,
        thread_id: &str,
        billing: &InferenceBillingRecord,
        status: CallStatus,
    ) -> Result<(), PureError> {
        self.record(
            root_thread_id,
            thread_id,
            billing,
            status,
            BillingRetention::Turn,
        )
    }

    pub(crate) fn record_auxiliary_inference(
        &self,
        root_thread_id: &str,
        thread_id: &str,
        billing: &InferenceBillingRecord,
        status: CallStatus,
    ) -> Result<(), PureError> {
        self.record(
            root_thread_id,
            thread_id,
            billing,
            status,
            BillingRetention::Internal,
        )
    }

    fn record(
        &self,
        root_thread_id: &str,
        thread_id: &str,
        billing: &InferenceBillingRecord,
        status: CallStatus,
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
                status,
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

    async fn read_projection(&self) -> Result<PerformanceProjection, PureError> {
        let calls = self.store.calls();
        let settled_before_read = calls.durable_ticket();
        let admitted = self.billing_ticket.load(Ordering::Acquire);
        // Statistics are an eventual, lossy projection: reads never wait for its writer.
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
        Ok(PerformanceProjection {
            global: StudioModelPerformanceSnapshot {
                revision,
                updated_at,
                statistics_pending: admitted > settled_before_read,
                statistics_gap: calls.statistics_gap(),
                read_failed: false,
                summaries: summaries.iter().map(summary_snapshot).collect(),
                history: samples.iter().map(history_sample).collect(),
            },
            session_costs: costs
                .iter()
                .filter(|rollup| !archived.contains(&rollup.root_thread_id))
                .map(session_cost_snapshot)
                .collect(),
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
                    owner.publish_projection().await;
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

    /// startup 在 `load_cache` 之后显式建立一次投影：已有调用事实立即成为最新首帧。
    ///
    /// 这是查询之外的前进点之一（startup 初始化 / 计费 durability 与 flags 变化 /
    /// 归档命令）；读取失败只影响展示 flags，不影响 runtime 启动。
    pub(crate) async fn initialize_projection(&self) {
        let _guard = self.project_lock.lock().await;
        self.publish_locked_projection().await;
    }

    /// 显式恢复目录后重投影费用可见性；不激活 Thread、不执行历史或修改计费事实。
    pub(crate) async fn refresh_directory_visibility(&self) {
        self.publish_projection().await;
    }

    /// 把一次投影读取发布到两个领域：全局性能事件 + 逐 root 作用域费用事件。
    ///
    /// 调用库读取、提交与发布在同 owner 内经 `project_lock` 串行：较旧的异步读取
    /// 不可能晚于较新的读取提交而覆盖新事实。
    async fn publish_projection(&self) {
        let _guard = self.project_lock.lock().await;
        self.publish_locked_projection().await;
    }

    /// 读取、在同一次 `published` 锁内原子采用、再在锁外按 typed topics 发布变化事实。
    ///
    /// 全局与作用域在同一次锁内成对采用：`snapshot_and_costs` 不会观察到新 global 搭配
    /// 旧 scoped 的撕裂组合；锁内不 send/await，发布事实全部在锁外发出。
    async fn publish_locked_projection(&self) {
        match self.read_projection().await {
            Ok(projection) => {
                let flags = (
                    projection.global.statistics_pending,
                    projection.global.statistics_gap,
                );
                let (global, scoped_events) = self
                    .commit_projection(projection.global, projection.session_costs, flags)
                    .await;
                let _ = self.product_events.emit_model_performance_state(global);
                for state in scoped_events {
                    self.product_events.emit_session_costs(state);
                }
            }
            Err(error) => {
                tracing::warn!(error = %error, "model performance projection read failed");
                let (pending, gap) = self.statistics_flags();
                let (global, scoped_events) = self.commit_read_failure(pending, gap).await;
                let _ = self.product_events.emit_model_performance_state(global);
                for state in scoped_events {
                    self.product_events.emit_session_costs(state);
                }
            }
        }
        // 有界产品对象（revision/更新时间）只在投影前进点写盘；查询不写盘。
        if let Err(error) = self.persist_state().await {
            tracing::warn!(error = %error, "model performance cache persist failed");
        }
    }

    fn revision(&self) -> u64 {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.revision
    }
}

/// 超出上限时淘汰最旧的已清除条目；有费用的条目不因容量被丢弃。
fn evict_cleared(scoped: &mut BTreeMap<String, ScopedCostEntry>) {
    if scoped.len() <= SCOPED_COST_LIMIT {
        return;
    }
    let mut cleared: Vec<(i64, String)> = scoped
        .iter()
        .filter(|(_, entry)| entry.cost.is_none())
        .map(|(root, entry)| (entry.updated_at, root.clone()))
        .collect();
    cleared.sort_unstable();
    let excess = scoped.len().saturating_sub(SCOPED_COST_LIMIT);
    for (_, root) in cleared.into_iter().take(excess) {
        scoped.remove(&root);
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
