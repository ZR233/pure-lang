use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

use super::backend::{
    CreateFailureDisposition, LocalWorktreeBackend, MergeOutcome, WorktreeBackend,
};
use super::error::WorktreeError;

/// worktree 目录在 repo 根下的相对位置。
const WORKTREE_DIR: &str = ".pure/worktrees";

/// worktree 分支前缀。
const WORKTREE_BRANCH_PREFIX: &str = "pure-agent-";

/// 随 subagent 生命周期绑定的 worktree 句柄，存入 `AgentEntry`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeHandle {
    pub path: PathBuf,
    pub branch: String,
}

/// 创建 worktree 时由宿主固定的仓库、路径、分支和基线。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeCreateSpec {
    pub repo_root: PathBuf,
    pub path: PathBuf,
    pub branch: String,
    pub base_commit: String,
}

/// 模型可见的 worktree 引用，用于 spawn 输出与 agent record。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeRef {
    pub path: String,
    pub branch: String,
}

impl From<&WorktreeHandle> for WorktreeRef {
    fn from(handle: &WorktreeHandle) -> Self {
        Self {
            path: handle.path.display().to_string(),
            branch: handle.branch.clone(),
        }
    }
}

/// close 时的产物处置方式。
///
/// worktree 生命周期严格等于 agent 生命周期：`close_agent` 是唯一释放点，
/// 且必须指明是把产物 merge 回主工作区还是直接丢弃。默认丢弃。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum CloseDisposition {
    /// 把 subagent 分支 merge 回主工作区当前分支，成功后释放 worktree。
    ///
    /// `target_branch` 预留用于将来指定 merge 目标分支；当前实现在主工作区
    /// 当前分支上执行 `git merge`。
    Merge { target_branch: Option<String> },
    /// 放弃修改，直接释放 worktree。
    #[default]
    Discard,
}

/// close 的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseOutcome {
    /// 产物已 merge 回主工作区，worktree 已释放。
    Merged,
    /// 产物已丢弃，worktree 已释放。
    Discarded,
}

/// per-subagent worktree 管理器。
///
/// 持有 repo_root 与 [`WorktreeBackend`]，负责路径分配、创建 / 合并 / 释放编排，
/// 以及孤儿 worktree 的启动 GC。默认 [`WorktreeManager::disabled`] 为 no-op，
/// 保持既有「subagent 共享 `workspace_root`」行为；显式启用后才分配 worktree。
#[derive(Debug, Clone)]
pub struct WorktreeManager {
    inner: Arc<ManagerInner>,
}

#[derive(Debug)]
struct ManagerInner {
    enabled: AtomicBool,
    repo_root: OnceLock<PathBuf>,
    backend: Arc<dyn WorktreeBackend>,
}

impl WorktreeManager {
    /// 构造未启用的 manager（no-op），向后兼容既有行为。
    pub fn disabled() -> Self {
        Self {
            inner: Arc::new(ManagerInner {
                enabled: AtomicBool::new(false),
                repo_root: OnceLock::new(),
                backend: Arc::new(LocalWorktreeBackend::new()),
            }),
        }
    }

    /// 构造已启用的 manager，使用默认本地 git 后端（主要供测试使用）。
    pub fn local(repo_root: PathBuf) -> Self {
        Self::with_backend(repo_root, Arc::new(LocalWorktreeBackend::new()))
    }

    /// 构造已启用的 manager，注入自定义后端。
    pub fn with_backend(repo_root: PathBuf, backend: Arc<dyn WorktreeBackend>) -> Self {
        Self {
            inner: Arc::new(ManagerInner {
                enabled: AtomicBool::new(true),
                repo_root: OnceLock::from(repo_root),
                backend,
            }),
        }
    }

    /// 幂等启用 worktree 支持：设置 repo_root 并在首次启用时清理孤儿。
    pub async fn enable(&self, repo_root: PathBuf) {
        let is_new = self.inner.repo_root.set(repo_root.clone()).is_ok();
        self.inner.enabled.store(true, Ordering::Release);
        if is_new {
            self.gc_orphans(&repo_root).await;
        }
    }

    /// worktree 支持是否已启用。
    pub fn is_enabled(&self) -> bool {
        self.inner.enabled.load(Ordering::Acquire)
    }

    /// 已启用的 repo_root。
    pub fn repo_root(&self) -> Option<PathBuf> {
        if self.is_enabled() {
            self.inner.repo_root.get().cloned()
        } else {
            None
        }
    }

    /// 分配 agent 对应的 worktree 路径。
    pub fn allocate_path(repo_root: &Path, agent_id: &str) -> PathBuf {
        repo_root.join(WORKTREE_DIR).join(agent_id)
    }

    /// 分配 agent 对应的分支名。
    pub fn branch_for(agent_id: &str) -> String {
        format!("{WORKTREE_BRANCH_PREFIX}{agent_id}")
    }

    /// 创建 agent 的 worktree。
    pub async fn create(&self, agent_id: &str) -> Result<WorktreeHandle, WorktreeError> {
        let repo_root = self.require_repo_root()?;
        self.create_from_spec(WorktreeCreateSpec {
            branch: Self::branch_for(agent_id),
            path: Self::allocate_path(&repo_root, agent_id),
            repo_root,
            base_commit: "HEAD".to_string(),
        })
        .await
    }

    /// 按宿主提供的精确 spec 创建 worktree。
    pub async fn create_from_spec(
        &self,
        mut spec: WorktreeCreateSpec,
    ) -> Result<WorktreeHandle, WorktreeError> {
        let configured_root = self.require_repo_root()?;
        if spec.repo_root != configured_root {
            return Err(WorktreeError::InvalidRepoRoot(format!(
                "spawn spec root {} does not match configured root {}",
                spec.repo_root.display(),
                configured_root.display()
            )));
        }
        let worktree_root = git_compatible_path(configured_root.join(WORKTREE_DIR));
        spec.path = git_compatible_path(spec.path);
        if spec.path.strip_prefix(&worktree_root).is_err() || spec.base_commit.trim().is_empty() {
            return Err(WorktreeError::InvalidRepoRoot(
                "spawn spec must use a non-empty base and a path below .pure/worktrees".to_string(),
            ));
        }
        let path = spec.path;
        let branch = spec.branch;
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|error| WorktreeError::Io(error.to_string()))?;
        }
        let handle = WorktreeHandle { path, branch };
        if let Err(failure) = self
            .inner
            .backend
            .create(
                &configured_root,
                &handle.branch,
                &handle.path,
                &spec.base_commit,
            )
            .await
        {
            let disposition = failure.disposition();
            let operation = failure.into_error();
            if disposition == CreateFailureDisposition::NoSideEffects {
                return Err(operation);
            }
            return match self.discard(&handle).await {
                Ok(()) => Err(operation),
                Err(cleanup) => Err(WorktreeError::OperationFailedWithCleanup {
                    operation: Box::new(operation),
                    cleanup: Box::new(cleanup),
                }),
            };
        }
        Ok(handle)
    }

    /// 按 disposition 释放 worktree；merge 冲突时不释放并返回错误。
    pub async fn close(
        &self,
        handle: &WorktreeHandle,
        disposition: CloseDisposition,
    ) -> Result<CloseOutcome, WorktreeError> {
        match disposition {
            CloseDisposition::Merge { target_branch } => {
                match self.merge(handle, target_branch.as_deref()).await? {
                    MergeOutcome::Merged => {
                        self.discard(handle).await?;
                        Ok(CloseOutcome::Merged)
                    }
                    MergeOutcome::Conflict => Err(WorktreeError::MergeConflict {
                        branch: handle.branch.clone(),
                        detail: String::new(),
                    }),
                }
            }
            CloseDisposition::Discard => {
                self.discard(handle).await?;
                Ok(CloseOutcome::Discarded)
            }
        }
    }

    async fn merge(
        &self,
        handle: &WorktreeHandle,
        _target_branch: Option<&str>,
    ) -> Result<MergeOutcome, WorktreeError> {
        let repo_root = self.require_repo_root()?;
        self.inner
            .backend
            .commit_all(&handle.path, "subagent worktree changes")
            .await?;
        self.inner
            .backend
            .merge_branch(&repo_root, &handle.branch)
            .await
    }

    /// 删除 worktree 与其分支，并聚合所有失败的清理步骤。
    async fn discard(&self, handle: &WorktreeHandle) -> Result<(), WorktreeError> {
        let repo_root = self.require_repo_root()?;
        let mut failures = Vec::new();
        if let Err(error) = self
            .inner
            .backend
            .remove(&repo_root, &handle.path, true)
            .await
        {
            failures.push(error);
        }

        match tokio::fs::try_exists(&handle.path).await {
            Ok(true) => {
                if let Err(error) = tokio::fs::remove_dir_all(&handle.path).await {
                    failures.push(WorktreeError::Io(format!(
                        "failed to remove {}: {error}",
                        handle.path.display()
                    )));
                }
            }
            Ok(false) => {}
            Err(error) => failures.push(WorktreeError::Io(format!(
                "failed to inspect {}: {error}",
                handle.path.display()
            ))),
        }

        if let Err(error) = self
            .inner
            .backend
            .delete_branch(&repo_root, &handle.branch)
            .await
        {
            failures.push(error);
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(WorktreeError::CleanupFailed {
                context: format!("worktree `{}`", handle.path.display()),
                failures,
            })
        }
    }

    /// 扫描 `.pure/worktrees/` 删除全部残留目录（会话刚启动时不存在存活 worktree）。
    async fn gc_orphans(&self, repo_root: &Path) {
        let dir = repo_root.join(WORKTREE_DIR);
        let mut entries = match tokio::fs::read_dir(&dir).await {
            Ok(entries) => entries,
            Err(_) => return,
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let _ = self.inner.backend.remove(repo_root, &path, true).await;
            if path.exists() {
                let _ = tokio::fs::remove_dir_all(&path).await;
            }
        }
    }

    fn require_repo_root(&self) -> Result<PathBuf, WorktreeError> {
        if !self.is_enabled() {
            return Err(WorktreeError::Disabled);
        }
        self.inner
            .repo_root
            .get()
            .cloned()
            .ok_or(WorktreeError::Disabled)
    }
}

pub(crate) fn git_compatible_path(path: PathBuf) -> PathBuf {
    if !cfg!(windows) {
        return path;
    }
    let path = path.to_string_lossy();
    if let Some(path) = path.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{path}"))
    } else if let Some(path) = path.strip_prefix(r"\\?\") {
        PathBuf::from(path)
    } else {
        PathBuf::from(path.as_ref())
    }
}
