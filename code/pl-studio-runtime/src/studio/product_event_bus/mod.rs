//! Studio 低频产品状态 owner 与事件通道：类型定义、事件发射与 revision 机械。
//!
//! Project 目录基线装载与应用在 `project`，Thread 目录热集合、冷分页 overlay
//! 与目录事实提交在 `thread_directory`，其余低频状态快照与事件发射在
//! `snapshots`。

mod project;
mod snapshots;
mod thread_directory;

use std::collections::{BTreeMap, HashMap};
use std::sync::{
    Arc,
    atomic::{AtomicI64, AtomicU64, Ordering},
};

use pl_protocol::{ObservedResource, Thread};
use tokio::sync::{Mutex, broadcast};

use crate::{
    PersistenceStateSnapshot, StudioAgentDirectoryEntry, StudioProductEventEnvelope,
    StudioProductEventKind,
};

use super::StudioStore;
use super::agent_host::ThreadWriteBehindWriter;
use super::ids::unix_seconds;

/// Studio 低频产品状态 owner 与事件通道。
///
/// 启动时以 SQLite 建立 Project 小集合基线；运行期间 Project、Thread 与
/// Agent 目录快照都由内存增量提交维护。所有 `read_*` 都是纯查询，活动事件不得
/// 回读数据库覆盖热事实。Thread 目录是"活动热集合 + SQLite 冷分页 overlay"：
/// `thread_index` 只保存仍有内存事实的 Thread，旧数据分页回源 SQLite。
#[derive(Clone)]
pub struct ProductEventBus {
    store: StudioStore,
    writer: ThreadWriteBehindWriter,
    tx: broadcast::Sender<StudioProductEventEnvelope>,
    sequence: Arc<AtomicU64>,
    revisions: Arc<ProductStateRevisions>,
    project_snapshot: Arc<Mutex<Vec<crate::ProjectRecord>>>,
    persistence_snapshot: Arc<std::sync::Mutex<PersistenceStateSnapshot>>,
    agents: Arc<Mutex<BTreeMap<String, StudioAgentDirectoryEntry>>>,
    /// 活动热集合（thread id → 列表元数据）：驻留、钉住或
    /// 目录 delta 尚未耐久化的 Thread；不含纯冷数据。
    thread_index: Arc<std::sync::Mutex<HashMap<String, Thread>>>,
}

#[derive(Default)]
struct ProductStateRevisions {
    project: DomainRevision,
    thread: DomainRevision,
    agent: DomainRevision,
    recovery: DomainRevision,
}

#[derive(Default)]
struct DomainRevision {
    revision: AtomicU64,
    updated_at: AtomicI64,
}

impl ProductEventBus {
    pub(in crate::studio) fn new(store: StudioStore, writer: ThreadWriteBehindWriter) -> Self {
        let (tx, _) = broadcast::channel(256);
        Self {
            store,
            writer,
            tx,
            sequence: Arc::new(AtomicU64::new(0)),
            revisions: Arc::new(ProductStateRevisions::default()),
            project_snapshot: Arc::new(Mutex::new(Vec::new())),
            persistence_snapshot: Arc::new(std::sync::Mutex::new(
                PersistenceStateSnapshot::default(),
            )),
            agents: Arc::new(Mutex::new(BTreeMap::new())),
            thread_index: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<StudioProductEventEnvelope> {
        self.tx.subscribe()
    }

    pub fn current_sequence(&self) -> u64 {
        self.sequence.load(Ordering::Acquire)
    }

    pub fn emit(&self, kind: StudioProductEventKind) -> StudioProductEventEnvelope {
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed) + 1;
        let envelope = StudioProductEventEnvelope {
            event_id: format!("studio-product-{sequence}"),
            sequence,
            created_at: unix_seconds(),
            kind,
        };
        let _ = self.tx.send(envelope.clone());
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

#[cfg(test)]
pub(super) mod tests {
    use crate::studio::StudioStore;
    use crate::studio::agent_host::ThreadWriteBehindWriter;

    use super::ProductEventBus;

    pub(in crate::studio::product_event_bus) async fn memory_bus() -> (StudioStore, ProductEventBus)
    {
        let store = StudioStore::open_memory().await.expect("memory store");
        let bus = ProductEventBus::new(store.clone(), ThreadWriteBehindWriter::new(store.clone()));
        bus.initialize_directories().await.expect("directories");
        (store, bus)
    }

    pub(in crate::studio::product_event_bus) async fn seed_project(
        store: &StudioStore,
    ) -> crate::ProjectRecord {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let workspace = std::env::temp_dir().join(format!("pure-directory-{unique}"));
        store.upsert_project(&workspace).await.expect("project")
    }
}
