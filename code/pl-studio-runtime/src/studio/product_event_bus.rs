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
    StudioLspStateSnapshot, StudioMcpStateSnapshot, StudioProductEventEnvelope,
    StudioProductEventKind, StudioProjectDirectoryData, StudioProjectDirectoryState,
    StudioRecoveryStateSnapshot, StudioSettingsStateSnapshot, StudioTaskDirectoryData,
    StudioTaskDirectoryEntry, StudioTaskDirectoryState, StudioThreadDirectoryData,
    StudioThreadDirectoryDelta, StudioThreadDirectoryPage, StudioThreadDirectoryPageData,
    StudioThreadDirectoryState, StudioUpdateStateSnapshot,
};

use super::{StudioStore, ids::unix_seconds};

/// Thread directory 分页的默认页大小上限。
const THREAD_DIRECTORY_PAGE_LIMIT: usize = 100;

/// Studio 低频产品状态 owner 与事件通道。
///
/// 启动时以 SQLite 建立恢复基线；运行期间 Project、Thread、Task 与 Agent 目录快照
/// 都由内存增量提交维护。所有 `read_*` 都是纯查询，活动事件不得回读数据库覆盖热事实。
#[derive(Clone)]
pub struct ProductEventBus {
    store: StudioStore,
    tx: broadcast::Sender<StudioProductEventEnvelope>,
    sequence: Arc<AtomicU64>,
    revisions: Arc<ProductStateRevisions>,
    task_snapshot: Arc<Mutex<Option<Vec<StudioTaskDirectoryEntry>>>>,
    project_snapshot: Arc<Mutex<Vec<crate::ProjectRecord>>>,
    persistence_snapshot: Arc<std::sync::Mutex<PersistenceStateSnapshot>>,
    agents: Arc<Mutex<BTreeMap<String, StudioAgentDirectoryEntry>>>,
    /// 常驻内存 Thread 目录索引（thread id → 列表元数据）。
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

impl ProductEventBus {
    pub fn new(store: StudioStore) -> Self {
        let (tx, _) = broadcast::channel(256);
        Self {
            store,
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

    /// 启动命令显式建立目录初始 revision 与内存目录索引；普通 read 不改变 revision。
    pub async fn initialize_directories(&self) -> Result<()> {
        self.initialize_revision(&self.revisions.project);
        self.initialize_revision(&self.revisions.thread);
        self.initialize_revision(&self.revisions.task);
        self.initialize_revision(&self.revisions.agent);
        self.initialize_revision(&self.revisions.recovery);
        *self.task_snapshot.lock().await = Some(Vec::new());
        *self.project_snapshot.lock().await = self.store.list_projects().await?;
        let index_threads = self.load_index_threads().await?;
        let mut index = self
            .thread_index
            .lock()
            .expect("thread index lock poisoned");
        index.clear();
        for thread in index_threads {
            index.insert(thread.id.clone(), thread);
        }
        Ok(())
    }

    async fn load_index_threads(&self) -> Result<Vec<Thread>> {
        let mut threads = Vec::new();
        for project in self.store.list_projects().await? {
            threads.extend(
                self.store
                    .list_threads(&project.id)
                    .await?
                    .into_iter()
                    .map(Into::into),
            );
        }
        Ok(threads)
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

    /// 从内存目录索引按 `(updatedAt, id)` 倒序 keyset 分页。
    pub async fn read_thread_directory_page(
        &self,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<StudioThreadDirectoryPage> {
        let limit = limit.clamp(1, THREAD_DIRECTORY_PAGE_LIMIT);
        let threads = self.sorted_thread_index();
        let remaining = match cursor.and_then(decode_thread_cursor) {
            Some(cursor_key) => threads
                .into_iter()
                .skip_while(|thread| thread_cursor_key(thread) >= cursor_key)
                .collect::<Vec<_>>(),
            None => threads,
        };
        let has_more = remaining.len() > limit;
        let page = remaining.into_iter().take(limit).collect::<Vec<_>>();
        let next_cursor = if has_more {
            page.last().map(encode_thread_cursor)
        } else {
            None
        };
        Ok(StudioThreadDirectoryPage {
            state: self.resource(
                &self.revisions.thread,
                StudioThreadDirectoryPageData {
                    threads: page,
                    next_cursor,
                },
            ),
        })
    }

    /// 应用一次目录增量并发布 `ThreadDirectoryChanged` 事件。
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

    /// 从常驻内存目录读取活动 Thread 元数据，不触发 SQLite 冷读取。
    pub(in crate::studio) fn thread_snapshot(&self, thread_id: &str) -> Option<Thread> {
        self.thread_index
            .lock()
            .expect("thread index lock poisoned")
            .get(thread_id)
            .cloned()
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
    ) -> Result<Option<StudioProductEventEnvelope>> {
        let mut previous = self.task_snapshot.lock().await;
        let tasks = previous.get_or_insert_default();
        if tasks
            .iter()
            .find(|task| task.root_thread_id == entry.root_thread_id)
            == Some(&entry)
        {
            return Ok(None);
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
        Ok(Some(self.emit(
            StudioProductEventKind::TaskDirectoryChanged(StudioTaskDirectoryState {
                state: self.resource(&self.revisions.task, StudioTaskDirectoryData { tasks }),
            }),
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

type ThreadCursorKey = (i64, String);

fn thread_cursor_key(thread: &Thread) -> ThreadCursorKey {
    (thread.updated_at, thread.id.clone())
}

fn encode_thread_cursor(thread: &Thread) -> String {
    format!("v1:{}:{}", thread.updated_at, thread.id)
}

fn decode_thread_cursor(cursor: &str) -> Option<ThreadCursorKey> {
    let rest = cursor.strip_prefix("v1:")?;
    let (updated_at, id) = rest.split_once(':')?;
    Some((updated_at.parse().ok()?, id.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn seed_directory_threads(count: i64) -> ProductEventBus {
        let store = StudioStore::open_memory().await.expect("memory store");
        let runtime = ProductEventBus::new(store.clone());
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let workspace = std::env::temp_dir().join(format!("pure-directory-{unique}"));
        let project = store.upsert_project(&workspace).await.expect("project");
        for index in 0..count {
            store
                .create_thread(
                    &project.id,
                    &format!("Session {index}"),
                    crate::StudioMode::Simple,
                )
                .await
                .expect("thread");
        }
        runtime.initialize_directories().await.expect("directories");
        runtime
    }

    #[tokio::test]
    async fn project_directory_changes_only_when_the_memory_owner_applies_a_fact() {
        let store = StudioStore::open_memory().await.expect("memory store");
        let runtime = ProductEventBus::new(store.clone());
        runtime.initialize_directories().await.expect("directories");
        let workspace = std::env::temp_dir().join("pure-project-memory-owner");
        let project = store.upsert_project(&workspace).await.expect("project");
        runtime
            .apply_project_entry(project.clone())
            .await
            .expect("hot project");
        store
            .quarantine_project(&project.id)
            .await
            .expect("cold archive");

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
    async fn thread_directory_page_walks_keyset_cursor_without_overlap() {
        let runtime = seed_directory_threads(7).await;
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

        let all = runtime.read_thread_directory().await.expect("full read");
        assert_eq!(all.state.value().expect("ready directory").threads.len(), 7);
    }

    #[tokio::test]
    async fn thread_delta_updates_index_and_emits_removed_ids() {
        let runtime = seed_directory_threads(1).await;
        let threads = runtime.read_thread_directory().await.expect("read");
        let thread = threads
            .state
            .value()
            .expect("ready directory")
            .threads
            .first()
            .expect("seeded thread")
            .clone();

        runtime
            .apply_thread_delta(Vec::new(), vec![thread.id.clone()])
            .await
            .expect("delta");
        let after = runtime.read_thread_directory().await.expect("read");
        assert!(
            after
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
        assert!(page.threads.is_empty());
        assert!(page.next_cursor.is_none());
    }
}
