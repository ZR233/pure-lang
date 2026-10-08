//! Studio 低频产品状态 owner 与事件通道：类型定义、事件发射与 revision 机械。
//!
//! Project 目录基线装载与应用在 `project`，Thread 目录热集合、冷分页 overlay
//! 与目录事实提交在 `thread_directory`，其余低频状态快照与事件发射在
//! `snapshots`，topic 通道注册与分发路由在 `topics`。

mod project;
mod snapshots;
mod thread_directory;
mod topics;

use std::collections::{BTreeMap, HashMap};
use std::sync::{
    Arc,
    atomic::{AtomicI64, AtomicU64, Ordering},
};

use pl_protocol::{ObservedResource, Thread};
use tokio::sync::{Mutex, broadcast};

use crate::{
    PersistenceStateSnapshot, StudioAgentDirectoryEntry, StudioPersistenceQueueStateSnapshot,
    StudioProductEventEnvelope, StudioProductEventKind, StudioProductTopic,
};

use super::StudioStore;
use super::agent_host::ThreadWriteBehindWriter;
use super::ids::unix_seconds;
use topics::TopicChannels;

/// Studio 低频产品状态 owner 与事件通道。
///
/// 启动时以 `workspaces.toml` 建立 Project 小集合基线；运行期间 Project、Thread 与
/// Agent 目录快照都由内存增量提交维护。所有 `read_*` 都是纯查询，活动事件不得
/// 回读数据库覆盖热事实。Thread 目录是"活动热集合 + `catalog.toml` 冷分页 overlay"：
/// `thread_index` 只保存仍有内存事实的 Thread，旧数据分页回源 `catalog.toml`；
/// SQLite `thread`/`project` 表只是 write-behind 镜像，运行期不作为第二读事实源。
///
/// GUI 订阅按 typed topic 登记（`subscribe_topic`）；runtime 内部消费者（如工具目录
/// 刷新）使用显式 `subscribe_internal`，不暴露无作用域的公开订阅。
#[derive(Clone)]
pub struct ProductEventBus {
    store: StudioStore,
    writer: ThreadWriteBehindWriter,
    /// runtime 内部消费者的显式混合通道；不面向 GUI transport。
    internal: broadcast::Sender<StudioProductEventEnvelope>,
    topics: Arc<std::sync::Mutex<TopicChannels>>,
    sequence: Arc<AtomicU64>,
    revisions: Arc<ProductStateRevisions>,
    project_snapshot: Arc<Mutex<Vec<crate::ProjectRecord>>>,
    persistence_snapshot: Arc<std::sync::Mutex<PersistenceStateSnapshot>>,
    persistence_queue: Arc<std::sync::Mutex<PersistenceQueuePublication>>,
    agents: Arc<Mutex<BTreeMap<String, StudioAgentDirectoryEntry>>>,
    /// 活动热集合（thread id → 列表元数据）：驻留、钉住或
    /// 目录 delta 尚未耐久化的 Thread；不含纯冷数据。
    thread_index: Arc<std::sync::Mutex<HashMap<String, Thread>>>,
    /// 已发布的 Mode catalog revision；按真变才发，避免重复装载空转发布。
    mode_catalog_revision: Arc<AtomicU64>,
}

#[derive(Default)]
struct ProductStateRevisions {
    project: DomainRevision,
    thread: DomainRevision,
    agent: DomainRevision,
}

#[derive(Default)]
struct DomainRevision {
    revision: AtomicU64,
    updated_at: AtomicI64,
}

/// 持久化队列的已发布事实；`published` 为 `None` 表示尚未发布过首帧。
#[derive(Default)]
struct PersistenceQueuePublication {
    revision: u64,
    published: Option<pl_protocol::PersistenceQueueSnapshot>,
    snapshot: StudioPersistenceQueueStateSnapshot,
}

impl ProductEventBus {
    pub(in crate::studio) fn new(store: StudioStore, writer: ThreadWriteBehindWriter) -> Self {
        let (internal, _) = broadcast::channel(256);
        Self {
            store,
            writer,
            internal,
            topics: Arc::new(std::sync::Mutex::new(TopicChannels::default())),
            sequence: Arc::new(AtomicU64::new(0)),
            revisions: Arc::new(ProductStateRevisions::default()),
            project_snapshot: Arc::new(Mutex::new(Vec::new())),
            persistence_snapshot: Arc::new(std::sync::Mutex::new(
                PersistenceStateSnapshot::default(),
            )),
            persistence_queue: Arc::new(std::sync::Mutex::new(
                PersistenceQueuePublication::default(),
            )),
            agents: Arc::new(Mutex::new(BTreeMap::new())),
            thread_index: Arc::new(std::sync::Mutex::new(HashMap::new())),
            mode_catalog_revision: Arc::new(AtomicU64::new(0)),
        }
    }

    /// 订阅一个 typed 产品 topic；通道在登记时创建，之后只接收该领域事件。
    pub fn subscribe_topic(
        &self,
        topic: &StudioProductTopic,
    ) -> broadcast::Receiver<StudioProductEventEnvelope> {
        self.topics
            .lock()
            .expect("product topic channels lock poisoned")
            .subscribe(topic)
    }

    /// runtime 内部消费者的显式订阅：接收全部产品事件。
    ///
    /// 该通道只服务 Studio runtime 自己的跨领域观察（工具目录刷新）；GUI transport
    /// 必须使用 typed topic 订阅，不得借用本通道后再在末端丢弃无关领域。
    pub(in crate::studio) fn subscribe_internal(
        &self,
    ) -> broadcast::Receiver<StudioProductEventEnvelope> {
        self.internal.subscribe()
    }

    pub fn current_sequence(&self) -> u64 {
        self.sequence.load(Ordering::Acquire)
    }

    /// 发布一条 canonical 事实：分配全局序列，按 topic 分发并同步内部通道。
    pub fn emit(&self, kind: StudioProductEventKind) -> StudioProductEventEnvelope {
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed) + 1;
        let envelope = StudioProductEventEnvelope {
            event_id: format!("studio-product-{sequence}"),
            sequence,
            created_at: unix_seconds(),
            kind,
        };
        let _ = self.internal.send(envelope.clone());
        self.topics
            .lock()
            .expect("product topic channels lock poisoned")
            .dispatch(&envelope);
        envelope
    }

    fn initialize_revision(&self, state: &DomainRevision) {
        if state
            .revision
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            state.updated_at.store(unix_seconds(), Ordering::Release);
        }
    }

    fn bump(&self, state: &DomainRevision) {
        self.initialize_revision(state);
        state.revision.fetch_add(1, Ordering::AcqRel);
        state.updated_at.store(unix_seconds(), Ordering::Release);
    }

    fn resource<T>(&self, state: &DomainRevision, value: T) -> ObservedResource<T> {
        let revision = state.revision.load(Ordering::Acquire);
        let updated_at = state.updated_at.load(Ordering::Acquire);
        if revision == 0 {
            ObservedResource::uninitialized(updated_at)
        } else {
            ObservedResource::ready(revision, updated_at, value)
        }
    }

    fn revision(&self, state: &DomainRevision) -> (u64, i64) {
        (
            state.revision.load(Ordering::Acquire),
            state.updated_at.load(Ordering::Acquire),
        )
    }
}
