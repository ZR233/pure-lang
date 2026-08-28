use std::collections::{BTreeMap, HashMap};
use std::sync::{
    Arc,
    atomic::{AtomicI64, AtomicU64, Ordering},
};

use anyhow::Result;
use pl_protocol::{ObservedResource, Thread};
use tokio::sync::{Mutex, broadcast, watch};

use crate::{
    PersistenceStateSnapshot, ProviderUsageStateSnapshot, SkillsStateSnapshot,
    StudioAgentDirectoryData, StudioAgentDirectoryEntry, StudioAgentDirectoryState,
    StudioLspStateSnapshot, StudioMcpStateSnapshot, StudioModelPerformanceSnapshot,
    StudioProductEventEnvelope, StudioProductEventKind, StudioProjectDirectoryData,
    StudioProjectDirectoryState, StudioRecoveryStateSnapshot, StudioSettingsStateSnapshot,
    StudioTaskDirectoryData, StudioTaskDirectoryEntry, StudioTaskDirectoryState,
    StudioThreadDirectoryData, StudioThreadDirectoryDelta, StudioThreadDirectoryPage,
    StudioThreadDirectoryPageData, StudioThreadDirectoryState, StudioUpdateStateSnapshot,
};

use super::StudioStore;
use super::agent_host::ThreadWriteBehindWriter;
use super::ids::unix_seconds;
use super::merged_page::{HotColdEntry, merge_page_desc};
use super::store::directory::{DirectoryDelta, RegisteredChildThread, ThreadDirectoryCursor};

/// Thread 目录分页的默认页大小上限。
const THREAD_DIRECTORY_PAGE_LIMIT: usize = 100;

/// Studio 低频产品状态 owner 与事件通道。
///
/// 启动时以 SQLite 建立 Project 小集合基线；运行期间 Project、Thread、Task 与
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
    task_snapshot: Arc<Mutex<Option<Vec<StudioTaskDirectoryEntry>>>>,
    project_snapshot: Arc<Mutex<Vec<crate::ProjectRecord>>>,
    persistence_snapshot: Arc<std::sync::Mutex<PersistenceStateSnapshot>>,
    agents: Arc<Mutex<BTreeMap<String, StudioAgentDirectoryEntry>>>,
    /// 活动热集合（thread id → 列表元数据）：驻留/钉住/活动 Task root 与
    /// 目录 delta 尚未耐久化的 Thread；不含纯冷数据。
    thread_index: Arc<std::sync::Mutex<HashMap<String, Thread>>>,
}

#[derive(Default)]
struct ProductStateRevisions {
    project: DomainRevision,
    thread: DomainRevision,
    task: DomainRevision,
    agent: DomainRevision,
    recovery: DomainRevision,
}

#[derive(Default)]
struct DomainRevision {
    revision: AtomicU64,
    updated_at: AtomicI64,
}

impl HotColdEntry for Thread {
    type Key = (i64, String);

    fn page_key(&self) -> Self::Key {
        (self.updated_at, self.id.clone())
    }

    fn entry_id(&self) -> &str {
        &self.id
    }
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
            task_snapshot: Arc::new(Mutex::new(None)),
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

    /// 启动命令显式建立目录初始 revision 与 Project 小集合；普通 read 不改变 revision。
    ///
    /// Thread 目录不做启动全量装载：活动热集合由钉住集合恢复、活动 Task root
    /// 与运行期目录 delta 构成，旧数据在分页查询时回源 SQLite。
    pub async fn initialize_directories(&self) -> Result<()> {
        self.initialize_revision(&self.revisions.project);
        self.initialize_revision(&self.revisions.thread);
        self.initialize_revision(&self.revisions.task);
        self.initialize_revision(&self.revisions.agent);
        self.initialize_revision(&self.revisions.recovery);
        self.task_snapshot.lock().await.get_or_insert_default();
        let durable_projects = self.store.list_projects().await?;
        let mut projects = self.project_snapshot.lock().await;
        for project in durable_projects {
            if !projects.iter().any(|hot| hot.id == project.id) {
                projects.push(project);
            }
        }
        projects.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| right.id.cmp(&left.id))
        });
        Ok(())
    }

    pub async fn read_project_directory(&self) -> Result<StudioProjectDirectoryState> {
        Ok(StudioProjectDirectoryState {
            state: self.resource(
                &self.revisions.project,
                StudioProjectDirectoryData {
                    projects: self.project_snapshot.lock().await.clone(),
                },
            ),
        })
    }

    pub(in crate::studio) async fn project_snapshot(&self) -> Vec<crate::ProjectRecord> {
        self.project_snapshot.lock().await.clone()
    }

    pub async fn read_thread_directory(&self) -> Result<StudioThreadDirectoryState> {
        Ok(StudioThreadDirectoryState {
            state: self.resource(
                &self.revisions.thread,
                StudioThreadDirectoryData {
                    threads: self.sorted_thread_index(),
                },
            ),
        })
    }

    /// 会话列表分页：SQLite 冷分页 + 活动热集合 overlay。
    ///
    /// 同 ID 热条目覆盖冷行，cursor 键排重；与 Turn 历史共用
    /// [`merge_page_desc`] 合并核心。热集合条目可能尚未耐久化，冷页查询
    /// 以 `limit + 1` 判定 has_more。
    pub async fn read_thread_directory_page(
        &self,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<StudioThreadDirectoryPage> {
        let limit = limit.clamp(1, THREAD_DIRECTORY_PAGE_LIMIT);
        let decoded = cursor.and_then(ThreadDirectoryCursor::decode);
        let cursor_key = decoded
            .as_ref()
            .map(|cursor| (cursor.updated_at, cursor.id.clone()));
        let cold = self
            .store
            .list_thread_directory_page(decoded.as_ref(), limit.saturating_add(1))
            .await?;
        let has_more = cold.len() > limit;
        let hot = self
            .thread_index
            .lock()
            .expect("thread index lock poisoned")
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let mut merged = merge_page_desc(hot, cold, cursor_key.as_ref());
        let has_more = has_more || merged.len() > limit;
        merged.truncate(limit);
        let next_cursor = has_more
            .then(|| {
                merged.last().map(|thread| ThreadDirectoryCursor {
                    updated_at: thread.updated_at,
                    id: thread.id.clone(),
                })
            })
            .flatten()
            .map(|cursor| cursor.encode());
        Ok(StudioThreadDirectoryPage {
            state: self.resource(
                &self.revisions.thread,
                StudioThreadDirectoryPageData {
                    threads: merged,
                    next_cursor,
                },
            ),
        })
    }

    /// 应用一次目录增量并发布 `ThreadDirectoryChanged` 事件（纯内存维护）。
    pub async fn apply_thread_delta(
        &self,
        upserted: Vec<Thread>,
        removed: Vec<String>,
    ) -> Result<StudioProductEventEnvelope> {
        {
            let mut index = self
                .thread_index
                .lock()
                .expect("thread index lock poisoned");
            for thread in &upserted {
                index.insert(thread.id.clone(), thread.clone());
            }
            for id in &removed {
                index.remove(id);
            }
        }
        self.bump(&self.revisions.thread);
        let (revision, updated_at) = self.revision(&self.revisions.thread);
        Ok(self.emit(StudioProductEventKind::ThreadDirectoryChanged(
            StudioThreadDirectoryDelta {
                revision,
                updated_at,
                upserted,
                removed,
            },
        )))
    }

    /// 提交一次目录事实：先加入 write-behind 队列，再更新内存热集合并广播。
    ///
    /// 这是 Thread/Project 目录 mutation 的唯一命令通道；admission 失败时不发布
    /// 半状态，后台 SQLite 失败只影响持久化健康状态。
    pub(in crate::studio) async fn commit_directory(
        &self,
        delta: DirectoryDelta,
    ) -> Result<StudioProductEventEnvelope> {
        self.writer.accept_directory(delta.clone())?;
        let (thread_upserts, thread_removals): (Vec<Thread>, Vec<String>) = (
            delta.thread_upserts.clone(),
            delta
                .thread_removals
                .iter()
                .flat_map(|removal| removal.thread_ids.iter().cloned())
                .chain(
                    delta
                        .project_removals
                        .iter()
                        .flat_map(|removal| removal.thread_ids.iter().cloned()),
                )
                .collect(),
        );
        let envelope = self
            .apply_thread_delta(thread_upserts, thread_removals)
            .await?;
        for project in &delta.project_upserts {
            self.apply_project_entry(crate::ProjectRecord {
                id: project.id.clone(),
                name: project.name.clone(),
                path: project.path.clone(),
                ssh_server_id: project.ssh_server_id.clone(),
                updated_at: project.updated_at,
            })
            .await?;
        }
        for removal in &delta.project_removals {
            self.remove_project_entry(&removal.project_id).await?;
        }
        Ok(envelope)
    }

    fn sorted_thread_index(&self) -> Vec<Thread> {
        let mut threads = self
            .thread_index
            .lock()
            .expect("thread index lock poisoned")
            .values()
            .cloned()
            .collect::<Vec<_>>();
        threads.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| right.id.cmp(&left.id))
        });
        threads
    }

    /// 从活动热集合读取 Thread 元数据；纯冷数据请走分页冷查询。
    pub(in crate::studio) fn thread_snapshot(&self, thread_id: &str) -> Option<Thread> {
        self.thread_index
            .lock()
            .expect("thread index lock poisoned")
            .get(thread_id)
            .cloned()
    }

    /// 注册一个 child Thread：typed delta 先完成 admission，热集合随后更新。
    pub(in crate::studio) async fn register_child_thread(
        &self,
        spec: RegisteredChildThread,
    ) -> Result<()> {
        let delta = DirectoryDelta::register_child_thread(spec);
        self.writer.accept_directory(delta.clone())?;
        self.apply_thread_delta(delta.thread_upserts.clone(), Vec::new())
            .await?;
        Ok(())
    }

    /// 热集合移除一个已耐久化且不再活动的 Thread 条目（LRU 淘汰路径）。
    pub(in crate::studio) fn evict_thread_entry(&self, thread_id: &str) {
        self.thread_index
            .lock()
            .expect("thread index lock poisoned")
            .remove(thread_id);
    }

    /// 热集合中属于指定 root 的全部条目（树归档时叠加尚未落库的 child）。
    pub(in crate::studio) fn threads_for_root(&self, root_thread_id: &str) -> Vec<Thread> {
        self.thread_index
            .lock()
            .expect("thread index lock poisoned")
            .values()
            .filter(|thread| thread.root_thread_id == root_thread_id)
            .cloned()
            .collect()
    }

    pub async fn read_task_directory(&self) -> Result<StudioTaskDirectoryState> {
        let tasks = self.task_snapshot.lock().await.clone().unwrap_or_default();
        Ok(StudioTaskDirectoryState {
            state: self.resource(&self.revisions.task, StudioTaskDirectoryData { tasks }),
        })
    }

    pub(in crate::studio) async fn initialize_task_directory(
        &self,
        tasks: Vec<StudioTaskDirectoryEntry>,
    ) {
        *self.task_snapshot.lock().await = Some(tasks);
    }

    pub async fn read_agent_directory(&self) -> StudioAgentDirectoryState {
        StudioAgentDirectoryState {
            state: self.resource(
                &self.revisions.agent,
                StudioAgentDirectoryData {
                    agents: self.agents.lock().await.values().cloned().collect(),
                },
            ),
        }
    }

    pub fn recovery_state(
        &self,
        issues: Vec<crate::StudioRecoveryIssue>,
    ) -> StudioRecoveryStateSnapshot {
        StudioRecoveryStateSnapshot {
            state: self.resource(&self.revisions.recovery, issues),
        }
    }

    /// 把调用方已经提交的 Project 事实直接应用到内存目录。
    pub async fn apply_project_entry(
        &self,
        project: crate::ProjectRecord,
    ) -> Result<StudioProductEventEnvelope> {
        let mut projects = self.project_snapshot.lock().await;
        if let Some(existing) = projects.iter_mut().find(|entry| entry.id == project.id) {
            *existing = project;
        } else {
            projects.push(project);
        }
        projects.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| right.id.cmp(&left.id))
        });
        drop(projects);
        self.bump(&self.revisions.project);
        let state = self.read_project_directory().await?;
        Ok(self.emit(StudioProductEventKind::ProjectDirectoryChanged(state)))
    }

    /// 从活动 Project 目录移除一个已归档或隔离的 Project。
    pub async fn remove_project_entry(
        &self,
        project_id: &str,
    ) -> Result<StudioProductEventEnvelope> {
        self.project_snapshot
            .lock()
            .await
            .retain(|project| project.id != project_id);
        self.bump(&self.revisions.project);
        let state = self.read_project_directory().await?;
        Ok(self.emit(StudioProductEventKind::ProjectDirectoryChanged(state)))
    }

    pub fn emit_agent_directory(
        &self,
        state: StudioAgentDirectoryState,
    ) -> StudioProductEventEnvelope {
        self.emit(StudioProductEventKind::AgentDirectoryChanged(state))
    }

    pub async fn update_agent_directory(
        &self,
        agent: StudioAgentDirectoryEntry,
    ) -> StudioProductEventEnvelope {
        self.agents.lock().await.insert(agent.id.clone(), agent);
        self.bump(&self.revisions.agent);
        self.emit_agent_directory(self.read_agent_directory().await)
    }

    /// 应用 TaskRuntime 已经提交的完整热投影。
    pub async fn apply_task_entry(
        &self,
        entry: StudioTaskDirectoryEntry,
    ) -> Option<StudioProductEventEnvelope> {
        let mut previous = self.task_snapshot.lock().await;
        let tasks = previous.get_or_insert_default();
        if tasks
            .iter()
            .find(|task| task.root_thread_id == entry.root_thread_id)
            == Some(&entry)
        {
            return None;
        }
        if let Some(existing) = tasks
            .iter_mut()
            .find(|task| task.root_thread_id == entry.root_thread_id)
        {
            *existing = entry;
        } else {
            tasks.push(entry);
        }
        tasks.sort_by(|left, right| left.root_thread_id.cmp(&right.root_thread_id));
        let tasks = tasks.clone();
        drop(previous);
        self.bump(&self.revisions.task);
        Some(self.emit(StudioProductEventKind::TaskDirectoryChanged(
            StudioTaskDirectoryState {
                state: self.resource(&self.revisions.task, StudioTaskDirectoryData { tasks }),
            },
        )))
    }

    pub async fn remove_task_entry(
        &self,
        root_thread_id: &str,
    ) -> Result<Option<StudioProductEventEnvelope>> {
        let mut previous = self.task_snapshot.lock().await;
        let tasks = previous.get_or_insert_default();
        let length = tasks.len();
        tasks.retain(|task| task.root_thread_id != root_thread_id);
        if tasks.len() == length {
            return Ok(None);
        }
        let tasks = tasks.clone();
        drop(previous);
        self.bump(&self.revisions.task);
        Ok(Some(self.emit(
            StudioProductEventKind::TaskDirectoryChanged(StudioTaskDirectoryState {
                state: self.resource(&self.revisions.task, StudioTaskDirectoryData { tasks }),
            }),
        )))
    }

    pub fn emit_settings_state(
        &self,
        state: StudioSettingsStateSnapshot,
    ) -> StudioProductEventEnvelope {
        self.emit(StudioProductEventKind::SettingsStateChanged(Box::new(
            state,
        )))
    }

    pub fn emit_recovery_state(
        &self,
        issues: Vec<crate::StudioRecoveryIssue>,
    ) -> StudioProductEventEnvelope {
        self.bump(&self.revisions.recovery);
        self.emit(StudioProductEventKind::RecoveryStateChanged(
            self.recovery_state(issues),
        ))
    }

    pub fn emit_mcp_state(&self, state: StudioMcpStateSnapshot) -> StudioProductEventEnvelope {
        self.emit(StudioProductEventKind::McpStateChanged(state))
    }

    pub fn emit_lsp_state(&self, state: StudioLspStateSnapshot) -> StudioProductEventEnvelope {
        self.emit(StudioProductEventKind::LspStateChanged(state))
    }

    pub fn emit_skills_state(&self, state: SkillsStateSnapshot) -> StudioProductEventEnvelope {
        self.emit(StudioProductEventKind::SkillsStateChanged(state.into()))
    }

    pub fn emit_provider_usage_state(
        &self,
        state: ProviderUsageStateSnapshot,
    ) -> StudioProductEventEnvelope {
        self.emit(StudioProductEventKind::ProviderUsageStateChanged(state))
    }

    pub fn emit_model_performance_state(
        &self,
        state: StudioModelPerformanceSnapshot,
    ) -> StudioProductEventEnvelope {
        self.emit(StudioProductEventKind::ModelPerformanceStateChanged(state))
    }

    pub fn emit_updater_state(
        &self,
        state: StudioUpdateStateSnapshot,
    ) -> StudioProductEventEnvelope {
        self.emit(StudioProductEventKind::UpdaterStateChanged(state))
    }

    pub fn persistence_state(&self) -> PersistenceStateSnapshot {
        self.persistence_snapshot
            .lock()
            .expect("persistence snapshot lock poisoned")
            .clone()
    }

    pub(in crate::studio) fn observe_persistence(
        &self,
        mut state: watch::Receiver<PersistenceStateSnapshot>,
    ) {
        let bus = self.clone();
        bus.update_persistence(state.borrow().clone());
        tokio::spawn(async move {
            while state.changed().await.is_ok() {
                bus.update_persistence(state.borrow_and_update().clone());
            }
        });
    }

    fn update_persistence(&self, state: PersistenceStateSnapshot) {
        let mut current = self
            .persistence_snapshot
            .lock()
            .expect("persistence snapshot lock poisoned");
        if state.revision <= current.revision {
            return;
        }
        *current = state.clone();
        drop(current);
        self.emit(StudioProductEventKind::PersistenceStateChanged(state));
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
mod tests {
    use super::super::agent_host::ThreadWriteBehindWriter;
    use super::*;

    async fn memory_bus() -> (StudioStore, ProductEventBus) {
        let store = StudioStore::open_memory().await.expect("memory store");
        let bus = ProductEventBus::new(store.clone(), ThreadWriteBehindWriter::new(store.clone()));
        bus.initialize_directories().await.expect("directories");
        (store, bus)
    }

    async fn seed_cold_threads(store: &StudioStore, project_id: &str, count: i64) {
        for index in 0..count {
            store
                .create_thread(
                    project_id,
                    &format!("Session {index}"),
                    crate::StudioMode::Simple,
                )
                .await
                .expect("thread");
        }
    }

    async fn seed_project(store: &StudioStore) -> crate::ProjectRecord {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let workspace = std::env::temp_dir().join(format!("pure-directory-{unique}"));
        store.upsert_project(&workspace).await.expect("project")
    }

    #[tokio::test]
    async fn project_directory_changes_only_when_the_memory_owner_applies_a_fact() {
        let (store, runtime) = memory_bus().await;
        let project = seed_project(&store).await;
        runtime
            .apply_project_entry(project.clone())
            .await
            .expect("hot project");

        let hot = runtime
            .read_project_directory()
            .await
            .expect("hot directory");
        assert_eq!(hot.state.value().unwrap().projects, vec![project.clone()]);

        runtime
            .remove_project_entry(&project.id)
            .await
            .expect("remove hot project");
        assert!(
            runtime
                .read_project_directory()
                .await
                .unwrap()
                .state
                .value()
                .unwrap()
                .projects
                .is_empty()
        );
    }

    #[tokio::test]
    async fn thread_directory_page_walks_cold_keyset_cursor_without_overlap() {
        let (store, runtime) = memory_bus().await;
        let project = seed_project(&store).await;
        seed_cold_threads(&store, &project.id, 7).await;

        let first = runtime
            .read_thread_directory_page(None, 3)
            .await
            .expect("first page");
        let first_data = first.state.value().expect("ready first page");
        assert_eq!(first_data.threads.len(), 3);
        let cursor = first_data.next_cursor.clone().expect("first page has more");

        let second = runtime
            .read_thread_directory_page(Some(&cursor), 3)
            .await
            .expect("second page");
        let second_data = second.state.value().expect("ready second page");
        assert_eq!(second_data.threads.len(), 3);
        assert_ne!(
            second_data.threads.first().unwrap().id,
            first_data.threads.last().unwrap().id
        );

        let cursor = second_data
            .next_cursor
            .clone()
            .expect("second page has more");
        let third = runtime
            .read_thread_directory_page(Some(&cursor), 3)
            .await
            .expect("third page");
        let third_data = third.state.value().expect("ready third page");
        assert_eq!(third_data.threads.len(), 1);
        assert!(third_data.next_cursor.is_none());
    }

    #[tokio::test]
    async fn hot_entries_overlay_cold_rows_and_fill_the_pending_window() {
        let (store, runtime) = memory_bus().await;
        let project = seed_project(&store).await;
        seed_cold_threads(&store, &project.id, 3).await;

        // 冷端第二页条目：一个被热事实覆盖，一个保持冷态。
        let cold = store
            .list_thread_directory_page(None, 10)
            .await
            .expect("cold page");
        let oldest = cold.last().expect("oldest cold thread").clone();
        let middle = cold[1].clone();
        let mut hot_overlay = middle.clone();
        hot_overlay.title = "hot refreshed".to_string();
        hot_overlay.updated_at += 100;
        // 尚未落库的新 Thread 只存在于热集合。
        let mut pending = oldest.clone();
        pending.id = format!("{}-new", pending.id);
        pending.agent_path = pending.id.clone();
        pending.updated_at += 200;

        runtime
            .apply_thread_delta(vec![hot_overlay.clone(), pending.clone()], Vec::new())
            .await
            .expect("hot delta");

        let page = runtime
            .read_thread_directory_page(None, 10)
            .await
            .expect("page");
        let threads = page.state.value().expect("ready page").threads.clone();
        assert_eq!(threads.len(), 4);
        // 热覆盖胜出且位于其新 key 位置；未落库条目参与排序。
        assert_eq!(threads.first().unwrap().id, pending.id);
        assert_eq!(threads[1].id, hot_overlay.id);
        assert_eq!(threads[1].title, "hot refreshed");
        // 同 id 冷行被覆盖，不重复出现。
        assert_eq!(threads.iter().filter(|t| t.id == hot_overlay.id).count(), 1);
    }

    #[tokio::test]
    async fn hot_removal_leaves_only_cold_entries_in_pages() {
        let (store, runtime) = memory_bus().await;
        let project = seed_project(&store).await;
        seed_cold_threads(&store, &project.id, 1).await;
        let cold = store
            .list_thread_directory_page(None, 10)
            .await
            .expect("cold page");
        let thread = cold.first().expect("seeded thread").clone();

        // 热集合移除（归档/淘汰）后条目回到纯冷态，仍由冷分页可见。
        runtime
            .apply_thread_delta(Vec::new(), vec![thread.id.clone()])
            .await
            .expect("delta");
        let hot_only = runtime.read_thread_directory().await.expect("hot read");
        assert!(
            hot_only
                .state
                .value()
                .expect("ready directory")
                .threads
                .is_empty()
        );

        let page = runtime
            .read_thread_directory_page(None, 10)
            .await
            .expect("page");
        let page = page.state.value().expect("ready page");
        assert_eq!(page.threads.len(), 1);
        assert_eq!(page.threads.first().unwrap().id, thread.id);
    }
}
