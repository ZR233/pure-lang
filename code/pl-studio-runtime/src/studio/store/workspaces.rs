//! `workspaces.toml`: Project/Workspace 目录事实的 canonical TOML 存储。
//!
//! 旧 `projects` 表属于旧会话版本，v2 不导入；普通目录读取只访问本文件。
//! 写路径与 catalog 一样使用单锁读取-修改-原子替换，`revision` 单调递增并保证同内容幂等。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use anyhow::{Context, Result, bail, ensure};

use crate::studio::records::ProjectRecord;
use crate::studio::store::StudioStore;
use crate::studio::store::directory::DirectoryDelta;

pub(in crate::studio) const WORKSPACES_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::studio) struct WorkspaceEntry {
    pub(in crate::studio) id: String,
    pub(in crate::studio) name: String,
    pub(in crate::studio) path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in crate::studio) ssh_alias: Option<String>,
    pub(in crate::studio) created_at: i64,
    pub(in crate::studio) updated_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in crate::studio) last_opened_at: Option<i64>,
    #[serde(default)]
    pub(in crate::studio) closed: bool,
}

impl WorkspaceEntry {
    pub(in crate::studio) fn project(&self) -> ProjectRecord {
        ProjectRecord {
            id: self.id.clone(),
            name: self.name.clone(),
            path: self.path.clone(),
            ssh_alias: self.ssh_alias.clone(),
            updated_at: self.updated_at,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkspacesDocument {
    schema_version: u32,
    revision: u64,
    workspaces: Vec<WorkspaceEntry>,
}

#[derive(Clone)]
pub(in crate::studio) struct WorkspaceStore {
    path: PathBuf,
    inner: Arc<Mutex<WorkspacesDocument>>,
}

impl WorkspaceStore {
    pub(in crate::studio) async fn load(path: PathBuf) -> Result<Self> {
        let document = match tokio::fs::read_to_string(&path).await {
            Ok(content) => decode(&path, &content)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => WorkspacesDocument {
                schema_version: WORKSPACES_SCHEMA_VERSION,
                revision: 1,
                workspaces: Vec::new(),
            },
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            path,
            inner: Arc::new(Mutex::new(document)),
        })
    }

    #[allow(dead_code)]
    pub(in crate::studio) fn revision(&self) -> u64 {
        self.lock().revision
    }

    pub(in crate::studio) fn entries(&self) -> Vec<WorkspaceEntry> {
        self.lock().workspaces.clone()
    }

    pub(in crate::studio) fn get(&self, id: &str) -> Option<WorkspaceEntry> {
        self.lock()
            .workspaces
            .iter()
            .find(|entry| entry.id == id)
            .cloned()
    }

    pub(in crate::studio) fn find_by_path(
        &self,
        path: &str,
        ssh_alias: Option<&str>,
    ) -> Option<WorkspaceEntry> {
        self.lock()
            .workspaces
            .iter()
            .find(|entry| entry.path == path && entry.ssh_alias.as_deref() == ssh_alias)
            .cloned()
    }

    pub(in crate::studio) async fn apply_delta(&self, delta: &DirectoryDelta) -> Result<()> {
        if delta.project_upserts.is_empty() && delta.project_removals.is_empty() {
            return Ok(());
        }
        let upserts = delta
            .project_upserts
            .iter()
            .map(|record| WorkspaceEntry {
                id: record.id.clone(),
                name: record.name.clone(),
                path: record.path.clone(),
                ssh_alias: record.ssh_alias.clone(),
                created_at: record.created_at,
                updated_at: record.updated_at,
                last_opened_at: record.last_opened_at,
                closed: record.closed,
            })
            .collect::<Vec<_>>();
        {
            let document = self.lock();
            for entry in &upserts {
                if let Some(existing) = document
                    .workspaces
                    .iter()
                    .find(|existing| existing.id == entry.id)
                {
                    ensure_identity_stable(existing, entry)?;
                }
            }
        }
        let removals = delta
            .project_removals
            .iter()
            .map(|removal| (removal.project_id.clone(), removal.closed_at))
            .collect::<Vec<_>>();
        self.mutate(move |document| {
            let mut changed = false;
            for entry in upserts {
                match document
                    .workspaces
                    .iter_mut()
                    .find(|existing| existing.id == entry.id)
                {
                    Some(existing)
                        if existing.path == entry.path && existing.ssh_alias == entry.ssh_alias =>
                    {
                        let next = WorkspaceEntry {
                            created_at: existing.created_at,
                            ..entry
                        };
                        if *existing != next {
                            *existing = next;
                            changed = true;
                        }
                    }
                    Some(_) => {}
                    None => {
                        document.workspaces.push(entry);
                        changed = true;
                    }
                }
            }
            for (id, closed_at) in removals {
                if let Some(existing) = document.workspaces.iter_mut().find(|entry| entry.id == id)
                    && (!existing.closed || existing.updated_at < closed_at)
                {
                    existing.closed = true;
                    existing.updated_at = existing.updated_at.max(closed_at);
                    changed = true;
                }
            }
            changed
        })
        .await
    }

    /// 把当前内存文档写入 canonical 文件；文件已存在时不覆盖。
    ///
    /// 只有 migration 边界使用，用来补齐旧发布遗留的空缺文档；正常运行写入都经
    /// [`Self::apply_delta`]。
    pub(in crate::studio) async fn persist_if_absent(&self) -> Result<()> {
        let inner = self.inner.clone();
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || {
            if path.exists() {
                return Ok(());
            }
            let document = inner.lock().unwrap_or_else(PoisonError::into_inner);
            let contents = toml::to_string_pretty(&*document)
                .context("failed to serialize Studio workspaces")?
                .into_bytes();
            pl_tool::workspace::write_file_atomically(&path, &contents).map_err(anyhow::Error::from)
        })
        .await?
    }

    async fn mutate(
        &self,
        change: impl FnOnce(&mut WorkspacesDocument) -> bool + Send + 'static,
    ) -> Result<()> {
        let inner = self.inner.clone();
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || mutate_document(&inner, &path, change)).await?
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, WorkspacesDocument> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn mutate_document(
    inner: &Mutex<WorkspacesDocument>,
    path: &Path,
    change: impl FnOnce(&mut WorkspacesDocument) -> bool,
) -> Result<()> {
    let mut document = inner.lock().unwrap_or_else(PoisonError::into_inner);
    let previous = document.clone();
    if !change(&mut document) {
        return Ok(());
    }
    document.revision = document.revision.saturating_add(1);
    let contents = toml::to_string_pretty(&*document)
        .context("failed to serialize Studio workspaces")?
        .into_bytes();
    match pl_tool::workspace::write_file_atomically(path, &contents) {
        Ok(()) => Ok(()),
        Err(error) => {
            *document = previous;
            Err(error.into())
        }
    }
}

fn decode(path: &Path, content: &str) -> Result<WorkspacesDocument> {
    let document: WorkspacesDocument = toml::from_str(content)
        .with_context(|| format!("invalid Studio workspaces {}", path.display()))?;
    ensure!(
        document.schema_version == WORKSPACES_SCHEMA_VERSION,
        "unsupported Studio workspaces schema in {}",
        path.display()
    );
    ensure!(
        document.revision >= 1,
        "Studio workspaces revision is invalid"
    );
    let mut seen = std::collections::BTreeSet::new();
    for entry in &document.workspaces {
        ensure!(
            !entry.id.is_empty() && !entry.path.is_empty(),
            "Studio workspace identity is incomplete"
        );
        ensure!(
            seen.insert(entry.id.as_str()),
            "Studio workspaces contain a duplicate Project identity"
        );
    }
    Ok(document)
}

impl StudioStore {
    pub(in crate::studio) fn workspaces(&self) -> &WorkspaceStore {
        &self.workspaces
    }
}

fn ensure_identity_stable(existing: &WorkspaceEntry, next: &WorkspaceEntry) -> Result<()> {
    if existing.path != next.path || existing.ssh_alias != next.ssh_alias {
        bail!(
            "Project {} directory identity changed: persisted path {}, delta path {}",
            existing.id,
            existing.path,
            next.path
        );
    }
    Ok(())
}
