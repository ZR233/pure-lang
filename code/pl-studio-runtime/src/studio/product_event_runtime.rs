use std::collections::BTreeMap;
use std::sync::{
    Arc,
    atomic::{AtomicI64, AtomicU64, Ordering},
};

use anyhow::Result;
use pl_protocol::ObservedStateMeta;
use tokio::sync::{Mutex, broadcast};

use crate::{
    ProviderUsageStateSnapshot, SkillsStateSnapshot, StudioAgentDirectoryEntry,
    StudioAgentDirectoryState, StudioLspStateSnapshot, StudioMcpStateSnapshot,
    StudioProductEventEnvelope, StudioProductEventKind, StudioProjectDirectoryState,
    StudioRecoveryStateSnapshot, StudioSettingsStateSnapshot, StudioTaskDirectoryEntry,
    StudioTaskDirectoryState, StudioThreadDirectoryState, StudioUpdateStateSnapshot,
};

use super::{StudioStore, ids::unix_seconds};

/// Studio 低频产品状态 owner 与事件通道。
///
/// SQLite 仍是 project/thread/task 的 canonical facts；本类型只持有各目录的单调 revision、
/// agent live directory 以及 transport。所有 `read_*` 都是纯查询。
#[derive(Clone)]
pub struct StudioProductEventRuntime {
    store: StudioStore,
    tx: broadcast::Sender<StudioProductEventEnvelope>,
    sequence: Arc<AtomicU64>,
    revisions: Arc<ProductStateRevisions>,
    task_snapshot: Arc<Mutex<Option<Vec<StudioTaskDirectoryEntry>>>>,
    agents: Arc<Mutex<BTreeMap<String, StudioAgentDirectoryEntry>>>,
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

impl StudioProductEventRuntime {
    pub fn new(store: StudioStore) -> Self {
        let (tx, _) = broadcast::channel(256);
        Self {
            store,
            tx,
            sequence: Arc::new(AtomicU64::new(0)),
            revisions: Arc::new(ProductStateRevisions::default()),
            task_snapshot: Arc::new(Mutex::new(None)),
            agents: Arc::new(Mutex::new(BTreeMap::new())),
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

    /// 启动命令显式建立目录初始 revision；普通 read 不会改变 revision。
    pub async fn initialize_directories(&self) -> Result<()> {
        self.initialize_revision(&self.revisions.project);
        self.initialize_revision(&self.revisions.thread);
        self.initialize_revision(&self.revisions.task);
        self.initialize_revision(&self.revisions.agent);
        self.initialize_revision(&self.revisions.recovery);
        *self.task_snapshot.lock().await = Some(self.load_tasks().await?);
        Ok(())
    }

    pub async fn read_project_directory(&self) -> Result<StudioProjectDirectoryState> {
        Ok(StudioProjectDirectoryState {
            meta: self.meta(&self.revisions.project),
            projects: self.store.list_projects().await?,
        })
    }

    pub async fn read_thread_directory(&self) -> Result<StudioThreadDirectoryState> {
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
        Ok(StudioThreadDirectoryState {
            meta: self.meta(&self.revisions.thread),
            threads,
        })
    }

    pub async fn read_task_directory(&self) -> Result<StudioTaskDirectoryState> {
        Ok(StudioTaskDirectoryState {
            meta: self.meta(&self.revisions.task),
            tasks: self.load_tasks().await?,
        })
    }

    pub async fn read_agent_directory(&self) -> StudioAgentDirectoryState {
        StudioAgentDirectoryState {
            meta: self.meta(&self.revisions.agent),
            agents: self.agents.lock().await.values().cloned().collect(),
        }
    }

    pub fn recovery_meta(&self) -> ObservedStateMeta {
        self.meta(&self.revisions.recovery)
    }

    pub async fn emit_project_directory(&self) -> Result<StudioProductEventEnvelope> {
        self.bump(&self.revisions.project);
        let state = self.read_project_directory().await?;
        Ok(self.emit(StudioProductEventKind::ProjectDirectoryChanged(state)))
    }

    pub async fn emit_thread_directory(&self) -> Result<StudioProductEventEnvelope> {
        self.bump(&self.revisions.thread);
        let state = self.read_thread_directory().await?;
        Ok(self.emit(StudioProductEventKind::ThreadDirectoryChanged(state)))
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
                meta: self.meta(&self.revisions.task),
                tasks,
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
            StudioRecoveryStateSnapshot {
                meta: self.recovery_meta(),
                issues,
            },
        ))
    }

    pub fn emit_mcp_state(&self, state: StudioMcpStateSnapshot) -> StudioProductEventEnvelope {
        self.emit(StudioProductEventKind::McpStateChanged(state))
    }

    pub fn emit_lsp_state(&self, state: StudioLspStateSnapshot) -> StudioProductEventEnvelope {
        self.emit(StudioProductEventKind::LspStateChanged(state))
    }

    pub fn emit_skills_state(&self, state: SkillsStateSnapshot) -> StudioProductEventEnvelope {
        self.emit(StudioProductEventKind::SkillsStateChanged(state))
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

    fn meta(&self, state: &DomainRevision) -> ObservedStateMeta {
        let revision = state.revision.load(Ordering::Acquire);
        let updated_at = state.updated_at.load(Ordering::Acquire);
        if revision == 0 {
            ObservedStateMeta::uninitialized(updated_at)
        } else {
            ObservedStateMeta::ready(revision, updated_at)
        }
    }
}
