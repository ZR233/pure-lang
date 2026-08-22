use std::collections::{BTreeMap, HashMap};
use std::sync::{
    Arc,
    atomic::{AtomicI64, AtomicU64, Ordering},
};

use anyhow::Result;
use pl_protocol::{ObservedResource, Thread};
use tokio::sync::{Mutex, broadcast};

use crate::{
    ProviderUsageStateSnapshot, SkillsStateSnapshot, StudioAgentDirectoryData,
    StudioAgentDirectoryEntry, StudioAgentDirectoryState, StudioLspStateSnapshot,
    StudioMcpStateSnapshot, StudioProductEventEnvelope, StudioProductEventKind,
    StudioProjectDirectoryData, StudioProjectDirectoryState, StudioRecoveryStateSnapshot,
    StudioSettingsStateSnapshot, StudioTaskDirectoryData, StudioTaskDirectoryEntry,
    StudioTaskDirectoryState, StudioThreadDirectoryData, StudioThreadDirectoryDelta,
    StudioThreadDirectoryPage, StudioThreadDirectoryPageData, StudioThreadDirectoryState,
    StudioUpdateStateSnapshot,
};

use super::{StudioStore, ids::unix_seconds};

/// Thread directory 分页的默认页大小上限。
const THREAD_DIRECTORY_PAGE_LIMIT: usize = 100;

/// Studio 低频产品状态 owner 与事件通道。
///
/// SQLite 是 project/thread/task 的 canonical facts；`threads` 的列表元数据在启动时
/// 建成常驻内存目录索引，此后由增量 mutation 同步维护——目录查询与
/// `ThreadDirectoryChanged` 事件都从索引派生，不重读数据库。其余目录只持有单调
/// revision、agent live directory 与 transport。所有 `read_*` 都是纯查询。
#[derive(Clone)]
pub struct ProductEventBus {
    store: StudioStore,
    tx: broadcast::Sender<StudioProductEventEnvelope>,
    sequence: Arc<AtomicU64>,
    revisions: Arc<ProductStateRevisions>,
    task_snapshot: Arc<Mutex<Option<Vec<StudioTaskDirectoryEntry>>>>,
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
        *self.task_snapshot.lock().await = Some(self.load_tasks().await?);
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
                    projects: self.store.list_projects().await?,
                },
            ),
        })
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

    /// 重读受影响 Thread 的 canonical 行并发布目录增量（归档/删除按移除处理）。
    ///
    /// 直接 store mutation（创建、归档、重命名、状态修复）之后调用；actor 的
    /// write-behind 提交由 observer 路径维护索引。
    pub async fn emit_thread_delta_for(&self, thread_ids: &[String]) -> Result<()> {
        let mut upserted = Vec::new();
        let mut removed = Vec::new();
        for id in thread_ids {
            match self.store.read_thread(id).await? {
                Some(record)
                    if record.visibility != crate::studio::records::ThreadVisibility::Archived =>
                {
                    upserted.push(record.into())
                }
                _ => removed.push(id.clone()),
            }
        }
        if upserted.is_empty() && removed.is_empty() {
            return Ok(());
        }
        self.apply_thread_delta(upserted, removed).await?;
        Ok(())
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

    pub async fn read_task_directory(&self) -> Result<StudioTaskDirectoryState> {
        Ok(StudioTaskDirectoryState {
            state: self.resource(
                &self.revisions.task,
                StudioTaskDirectoryData {
                    tasks: self.load_tasks().await?,
                },
            ),
        })
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

    pub async fn emit_project_directory(&self) -> Result<StudioProductEventEnvelope> {
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

    /// 重新读取并在内容真正变化时发布完整 task directory。
    pub async fn refresh_task(
        &self,
        _root_thread_id: &str,
    ) -> Result<Option<StudioProductEventEnvelope>> {
        let tasks = self.load_tasks().await?;
        let mut previous = self.task_snapshot.lock().await;
        if previous.as_ref() == Some(&tasks) {
            return Ok(None);
        }
        *previous = Some(tasks.clone());
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

    async fn load_tasks(&self) -> Result<Vec<StudioTaskDirectoryEntry>> {
        let mut tasks = Vec::new();
        for project in self.store.list_projects().await? {
            for thread in self.store.list_root_threads(&project.id).await? {
                if let Some(task) =
                    super::task_projection::load_task_runtime(&self.store, &thread.id).await?
                {
                    tasks.push(StudioTaskDirectoryEntry {
                        root_thread_id: thread.id,
                        task,
                    });
                }
            }
        }
        tasks.sort_by(|left, right| left.root_thread_id.cmp(&right.root_thread_id));
        Ok(tasks)
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
