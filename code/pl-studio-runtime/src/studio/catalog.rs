//! `catalog.toml`: 轻量会话目录摘要的唯一事实源。
//!
//! 首屏列表、目录分页与搜索只从本文件构造 `Thread` 摘要，不读取任何会话
//! `state.toml` 或 `history.sqlite`（design/17 §17.1、§17.2）。因此条目只保留
//! 目录展示与 `ThreadDirectoryQuery` 判定所需的稳定字段：身份、标题、Mode、角色、
//! 工作区模式/地址、时间戳、归档状态与有界活动摘要。当前上下文、Turn、历史正文、
//! 请求/工具结果与执行状态一律不在其中，活动指示器由内存热集合覆盖。
//!
//! 文件使用版本化 schema、CAS revision 与原子写入；运行期热集合（`ProductEventBus`）
//! 仍在内存中先行，本模块只承载已提交为目录事实的冷摘要。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use anyhow::{Context, Result, bail, ensure};
use pl_protocol::studio::ThreadDirectoryQuery;
use pl_protocol::{Thread, ThreadModeId, ThreadStatus, ThreadWorkspaceMode};

pub(in crate::studio) const CATALOG_SCHEMA_VERSION: u32 = 1;

/// 一个会话的轻量目录摘要。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::studio) struct CatalogEntry {
    pub(in crate::studio) id: String,
    pub(in crate::studio) project_id: String,
    pub(in crate::studio) title: String,
    pub(in crate::studio) mode: ThreadModeId,
    pub(in crate::studio) workspace_mode: ThreadWorkspaceMode,
    pub(in crate::studio) workspace_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in crate::studio) parent_thread_id: Option<String>,
    pub(in crate::studio) root_thread_id: String,
    pub(in crate::studio) role: String,
    pub(in crate::studio) agent_path: String,
    pub(in crate::studio) status: ThreadStatus,
    pub(in crate::studio) created_at: i64,
    pub(in crate::studio) updated_at: i64,
    pub(in crate::studio) archived: bool,
}

impl CatalogEntry {
    pub(in crate::studio) fn from_thread(thread: &Thread) -> Self {
        Self {
            id: thread.id.clone(),
            project_id: thread.project_id.clone(),
            title: thread.title.clone(),
            mode: thread.mode.clone(),
            workspace_mode: thread.workspace_mode,
            workspace_path: thread.workspace_path.clone(),
            parent_thread_id: thread.parent_thread_id.clone(),
            root_thread_id: thread.root_thread_id.clone(),
            role: thread.role.clone(),
            agent_path: thread.agent_path.clone(),
            status: thread.status,
            created_at: thread.created_at,
            updated_at: thread.updated_at,
            archived: thread.archived,
        }
    }

    pub(in crate::studio) fn to_thread(&self) -> Thread {
        Thread {
            id: self.id.clone(),
            project_id: self.project_id.clone(),
            title: self.title.clone(),
            mode: self.mode.clone(),
            workspace_mode: self.workspace_mode,
            workspace_path: self.workspace_path.clone(),
            root_thread_id: self.root_thread_id.clone(),
            parent_thread_id: self.parent_thread_id.clone(),
            role: self.role.clone(),
            agent_path: self.agent_path.clone(),
            status: self.status,
            created_at: self.created_at,
            updated_at: self.updated_at,
            archived: self.archived,
        }
    }

    /// 与 `pl_protocol::ThreadDirectoryQuery::matches` 同一判定语义，供冷分页与搜索使用。
    ///
    /// `project_matches` 由调用方按 Project 名称/路径命中集合传入；目录自身不读取 Project。
    pub(in crate::studio) fn matches(
        &self,
        query: &ThreadDirectoryQuery,
        project_matches: bool,
    ) -> bool {
        self.parent_thread_id.is_none()
            && self.archived == query.archived
            && query
                .project_id
                .as_ref()
                .is_none_or(|id| *id == self.project_id)
            && (project_matches
                || query.search.as_ref().is_none_or(|text| {
                    self.title
                        .to_lowercase()
                        .contains(&text.trim().to_lowercase())
                }))
            && match query.filter {
                pl_protocol::studio::ThreadDirectoryFilter::All => true,
                pl_protocol::studio::ThreadDirectoryFilter::Running => matches!(
                    self.status,
                    ThreadStatus::Queued
                        | ThreadStatus::Running
                        | ThreadStatus::WaitingTool
                        | ThreadStatus::Cancelling
                ),
                pl_protocol::studio::ThreadDirectoryFilter::Attention => matches!(
                    self.status,
                    ThreadStatus::WaitingInteraction | ThreadStatus::Faulted
                ),
            }
    }
}

/// `catalog.toml` 的持久化文档。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CatalogDocument {
    schema_version: u32,
    revision: u64,
    entries: Vec<CatalogEntry>,
}

/// 目录摘要的进程内 owner：文件是唯一事实源，内存缓存避免分页时的重复解析。
///
/// 所有 mutation 都走同一把锁内的读取-修改-原子替换，`revision` 单调递增用于
/// CAS 语义；读路径返回缓存克隆，不读取会话状态。
#[derive(Clone)]
pub(in crate::studio) struct CatalogStore {
    path: PathBuf,
    inner: Arc<Mutex<CatalogDocument>>,
}

impl CatalogStore {
    /// 从磁盘加载（缺失时为带 revision 1 的空目录）。
    pub(in crate::studio) async fn load(path: PathBuf) -> Result<Self> {
        let document = match tokio::fs::read_to_string(&path).await {
            Ok(content) => decode(&path, &content)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => CatalogDocument {
                schema_version: CATALOG_SCHEMA_VERSION,
                revision: 1,
                entries: Vec::new(),
            },
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            path,
            inner: Arc::new(Mutex::new(document)),
        })
    }

    /// 全部条目的稳定快照（按 `(updated_at, id)` 降序）。
    pub(in crate::studio) fn entries(&self) -> Vec<CatalogEntry> {
        let mut entries = self.lock().entries.clone();
        sort_desc(&mut entries);
        entries
    }

    pub(in crate::studio) fn get(&self, thread_id: &str) -> Option<CatalogEntry> {
        self.lock()
            .entries
            .iter()
            .find(|entry| entry.id == thread_id)
            .cloned()
    }

    /// 冷目录分页：`(updated_at, id)` 降序 keyset；查询命中后才应用 cursor，保证搜索语义。
    pub(in crate::studio) fn page(
        &self,
        query: &ThreadDirectoryQuery,
        project_matches: &std::collections::HashSet<String>,
        cursor: Option<&(i64, String)>,
        limit: usize,
    ) -> Vec<Thread> {
        let mut matched = self
            .lock()
            .entries
            .iter()
            .filter(|entry| entry.matches(query, project_matches.contains(&entry.project_id)))
            .cloned()
            .collect::<Vec<_>>();
        sort_desc(&mut matched);
        matched
            .into_iter()
            .filter(|entry| {
                cursor.is_none_or(|(updated_at, id)| {
                    (entry.updated_at, entry.id.clone()) < (*updated_at, id.clone())
                })
            })
            .take(limit)
            .map(|entry| entry.to_thread())
            .collect()
    }

    /// 未归档 root Thread 的冷分页（无搜索过滤）。
    pub(in crate::studio) fn page_active(
        &self,
        cursor: Option<&(i64, String)>,
        limit: usize,
    ) -> Vec<Thread> {
        let mut matched = self
            .lock()
            .entries
            .iter()
            .filter(|entry| !entry.archived && entry.parent_thread_id.is_none())
            .cloned()
            .collect::<Vec<_>>();
        sort_desc(&mut matched);
        matched
            .into_iter()
            .filter(|entry| {
                cursor.is_none_or(|(updated_at, id)| {
                    (entry.updated_at, entry.id.clone()) < (*updated_at, id.clone())
                })
            })
            .take(limit)
            .map(|entry| entry.to_thread())
            .collect()
    }

    /// 幂等写入/替换一个条目；内容不变时不推进 revision，也不落盘。
    pub(in crate::studio) async fn upsert(&self, entry: CatalogEntry) -> Result<()> {
        self.mutate(move |document| {
            match document
                .entries
                .iter_mut()
                .find(|existing| existing.id == entry.id)
            {
                Some(existing) if *existing == entry => false,
                Some(existing) => {
                    *existing = entry;
                    true
                }
                None => {
                    document.entries.push(entry);
                    true
                }
            }
        })
        .await
    }

    /// 幂等标记归档；条目缺失或已归档时不落盘。
    pub(in crate::studio) async fn archive(
        &self,
        thread_ids: &[String],
        archived_at: i64,
    ) -> Result<()> {
        let thread_ids = thread_ids.to_vec();
        self.mutate(move |document| {
            let mut changed = false;
            for id in &thread_ids {
                if let Some(entry) = document.entries.iter_mut().find(|entry| entry.id == *id)
                    && !entry.archived
                {
                    entry.archived = true;
                    entry.updated_at = entry.updated_at.max(archived_at);
                    changed = true;
                }
            }
            changed
        })
        .await
    }

    /// 幂等推进活动时间戳；缺失条目 no-op。
    pub(in crate::studio) async fn touch(&self, thread_ids: &[(String, i64)]) -> Result<()> {
        let thread_ids = thread_ids.to_vec();
        self.mutate(move |document| {
            let mut changed = false;
            for (id, updated_at) in &thread_ids {
                if let Some(entry) = document.entries.iter_mut().find(|entry| entry.id == *id) {
                    let next = entry.updated_at.max(*updated_at);
                    if next != entry.updated_at {
                        entry.updated_at = next;
                        changed = true;
                    }
                }
            }
            changed
        })
        .await
    }

    /// 把当前内存文档写入 canonical 文件；文件已存在时不覆盖。
    ///
    /// 新安装启动时确保 `catalog.toml` 存在，以便后续缺失被识别为数据丢失。
    pub(in crate::studio) async fn persist_if_absent(&self) -> Result<()> {
        let inner = self.inner.clone();
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || {
            if path.exists() {
                return Ok(());
            }
            let document = inner.lock().unwrap_or_else(PoisonError::into_inner);
            let contents = toml::to_string_pretty(&*document)
                .context("failed to serialize Studio catalog")?
                .into_bytes();
            pl_tool::workspace::write_file_atomically(&path, &contents).map_err(anyhow::Error::from)
        })
        .await?
    }

    async fn mutate(
        &self,
        change: impl FnOnce(&mut CatalogDocument) -> bool + Send + 'static,
    ) -> Result<()> {
        let inner = self.inner.clone();
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || mutate_document(&inner, &path, change)).await?
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, CatalogDocument> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn mutate_document(
    inner: &Mutex<CatalogDocument>,
    path: &Path,
    change: impl FnOnce(&mut CatalogDocument) -> bool,
) -> Result<()> {
    let mut document = inner.lock().unwrap_or_else(PoisonError::into_inner);
    let previous = document.clone();
    if !change(&mut document) {
        return Ok(());
    }
    document.revision = document.revision.saturating_add(1);
    let contents = toml::to_string_pretty(&*document)
        .context("failed to serialize Studio catalog")?
        .into_bytes();
    match pl_tool::workspace::write_file_atomically(path, &contents) {
        Ok(()) => Ok(()),
        Err(error) => {
            *document = previous;
            Err(error.into())
        }
    }
}

fn decode(path: &Path, content: &str) -> Result<CatalogDocument> {
    let document: CatalogDocument = toml::from_str(content)
        .with_context(|| format!("invalid Studio catalog {}", path.display()))?;
    ensure!(
        document.schema_version == CATALOG_SCHEMA_VERSION,
        "unsupported Studio catalog schema in {}",
        path.display()
    );
    ensure!(document.revision >= 1, "Studio catalog revision is invalid");
    let mut seen = std::collections::BTreeSet::new();
    for entry in &document.entries {
        ensure!(
            seen.insert(entry.id.as_str()),
            "Studio catalog has a duplicate Thread identity"
        );
        ensure!(
            !entry.id.is_empty() && entry.agent_path == entry.id,
            "Studio catalog entry identity is inconsistent"
        );
        ensure!(
            entry.workspace_mode != ThreadWorkspaceMode::Worktree
                || !entry.workspace_path.is_empty(),
            "Studio worktree catalog entry has no workspace address"
        );
        if entry.parent_thread_id.is_none() {
            ensure!(
                entry.root_thread_id == entry.id,
                "Studio root catalog entry is not its own root"
            );
        }
    }
    if document.entries.is_empty() && document.revision > 1 {
        bail!("Studio catalog lost all entries without a fresh install");
    }
    Ok(document)
}

fn sort_desc(entries: &mut [CatalogEntry]) {
    entries.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| right.id.cmp(&left.id))
    });
}
