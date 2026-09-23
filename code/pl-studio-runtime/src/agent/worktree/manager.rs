use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{WorktreeBackend, WorktreeCreateFailureDisposition, WorktreeError, WorktreeStatus};

const WORKTREE_DIR: &str = ".anywork/worktrees";
const CHILD_BRANCH_PREFIX: &str = "pure-agent-";
const SESSION_BRANCH_PREFIX: &str = "pure-session-";
/// 根会话自身工作区使用的受控 leaf 名称。
pub const SESSION_WORKTREE_LEAF: &str = "session";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeHandle {
    pub path: PathBuf,
    pub branch: String,
    pub base_commit: String,
}

/// Pure-owned worktree 归属；路径 leaf 与分支名都由归属派生，不接受外部指定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorktreeOwnership {
    /// 根会话自身工作区：`<repo>/.anywork/worktrees/<root-thread-id>/session` 与
    /// `pure-session-<thread-id>`。
    Session { thread_id: String },
    /// 子智能体工作区：`<repo>/.anywork/worktrees/<root-thread-id>/<child-id>` 与
    /// `pure-agent-<child-id>`。
    Child { child_id: String },
}

impl WorktreeOwnership {
    pub fn leaf(&self) -> &str {
        match self {
            Self::Session { .. } => SESSION_WORKTREE_LEAF,
            Self::Child { child_id } => child_id,
        }
    }

    pub fn owner_id(&self) -> &str {
        match self {
            Self::Session { thread_id } => thread_id,
            Self::Child { child_id } => child_id,
        }
    }

    pub fn branch(&self) -> String {
        match self {
            Self::Session { thread_id } => {
                format!("{SESSION_BRANCH_PREFIX}{}", safe_component(thread_id))
            }
            Self::Child { child_id } => {
                format!("{CHILD_BRANCH_PREFIX}{}", safe_component(child_id))
            }
        }
    }
}

/// Rejects any branch that is not a Pure-owned registration identity.
pub fn is_pure_branch(branch: &str) -> bool {
    branch.starts_with(CHILD_BRANCH_PREFIX) || branch.starts_with(SESSION_BRANCH_PREFIX)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeCreateSpec {
    pub repo_root: PathBuf,
    pub root_thread_id: String,
    pub ownership: WorktreeOwnership,
    pub base_commit: String,
}

#[derive(Debug, Clone)]
pub struct WorktreeManager {
    repo_root: PathBuf,
    backend: Arc<dyn WorktreeBackend>,
}

impl WorktreeManager {
    pub fn new(repo_root: PathBuf, backend: Arc<dyn WorktreeBackend>) -> Self {
        Self { repo_root, backend }
    }

    pub async fn resolve_repository_root(
        backend: &dyn WorktreeBackend,
        project_path: &Path,
    ) -> Result<PathBuf, WorktreeError> {
        backend.resolve_repo_root(project_path).await
    }

    pub fn allocate_path(
        repo_root: &Path,
        root_thread_id: &str,
        ownership: &WorktreeOwnership,
    ) -> PathBuf {
        repo_root
            .join(WORKTREE_DIR)
            .join(safe_component(root_thread_id))
            .join(safe_component(ownership.leaf()))
    }

    pub fn branch_for(ownership: &WorktreeOwnership) -> String {
        ownership.branch()
    }

    pub async fn resolve_head(&self, path: &Path) -> Result<String, WorktreeError> {
        self.backend.resolve_head(path).await
    }

    pub async fn preview(&self, handle: &WorktreeHandle) -> Result<WorktreeStatus, WorktreeError> {
        self.validate_handle(handle)?;
        self.backend.status(&handle.path).await
    }

    /// Previews a remaining owned worktree while allowing cleanup retries after directory removal.
    ///
    /// # Errors
    /// Rejects invalid owned-leaf identity, failed existence checks and unreadable existing worktrees.
    pub async fn preview_existing(
        &self,
        handle: &WorktreeHandle,
    ) -> Result<Option<WorktreeStatus>, WorktreeError> {
        self.validate_handle(handle)?;
        if self.backend.path_exists(&handle.path).await? {
            self.backend.status(&handle.path).await.map(Some)
        } else {
            Ok(None)
        }
    }

    pub async fn create(&self, spec: WorktreeCreateSpec) -> Result<WorktreeHandle, WorktreeError> {
        if spec.repo_root != self.repo_root || spec.base_commit.trim().is_empty() {
            return Err(WorktreeError::InvalidResource(
                "worktree spawn spec has mismatched repository or empty base".to_string(),
            ));
        }
        let path = Self::allocate_path(&self.repo_root, &spec.root_thread_id, &spec.ownership);
        let branch = Self::branch_for(&spec.ownership);
        let expected_parent = self
            .repo_root
            .join(WORKTREE_DIR)
            .join(safe_component(&spec.root_thread_id));
        if path.parent() != Some(expected_parent.as_path()) {
            return Err(WorktreeError::InvalidResource(
                "worktree target is not an exact Pure-owned leaf".to_string(),
            ));
        }
        self.backend.create_parent(&self.repo_root, &path).await?;
        let handle = WorktreeHandle {
            path,
            branch,
            base_commit: spec.base_commit,
        };
        if let Err(failure) = self
            .backend
            .create(
                &self.repo_root,
                &handle.branch,
                &handle.path,
                &handle.base_commit,
            )
            .await
        {
            let disposition = failure.disposition();
            let operation = failure.into_error();
            if disposition == WorktreeCreateFailureDisposition::NoSideEffects {
                return Err(operation);
            }
            return match self.discard(&handle).await {
                Ok(()) => Err(WorktreeError::OperationFailedAfterCleanup {
                    operation: Box::new(operation),
                }),
                Err(cleanup) => Err(WorktreeError::OperationFailedWithCleanup {
                    operation: Box::new(operation),
                    cleanup: Box::new(cleanup),
                }),
            };
        }
        let verification = self
            .backend
            .resolve_head(&handle.path)
            .await
            .and_then(|actual| {
                if actual == handle.base_commit {
                    Ok(())
                } else {
                    Err(WorktreeError::InvalidResource(format!(
                        "created worktree HEAD {actual} does not match frozen base {}",
                        handle.base_commit
                    )))
                }
            });
        if let Err(operation) = verification {
            return match self.discard(&handle).await {
                Ok(()) => Err(WorktreeError::OperationFailedAfterCleanup {
                    operation: Box::new(operation),
                }),
                Err(cleanup) => Err(WorktreeError::OperationFailedWithCleanup {
                    operation: Box::new(operation),
                    cleanup: Box::new(cleanup),
                }),
            };
        }
        Ok(handle)
    }

    fn validate_handle(&self, handle: &WorktreeHandle) -> Result<(), WorktreeError> {
        let expected_root = self.repo_root.join(WORKTREE_DIR);
        if !handle.path.starts_with(&expected_root)
            || handle.path.components().count() != expected_root.components().count() + 2
            || !is_pure_branch(&handle.branch)
        {
            return Err(WorktreeError::InvalidResource(
                "cleanup refused a non-Pure or non-leaf worktree identity".to_string(),
            ));
        }
        Ok(())
    }

    pub async fn discard(&self, handle: &WorktreeHandle) -> Result<(), WorktreeError> {
        self.validate_handle(handle)?;
        let registration_error = self
            .backend
            .remove(&self.repo_root, &handle.path, true)
            .await
            .err();
        let leaf_error = match self.backend.path_exists(&handle.path).await {
            Ok(true) if registration_error.is_some() => {
                // A lock or ownership refusal must not be bypassed by filesystem deletion.
                return Err(WorktreeError::CleanupFailed {
                    context: handle.path.display().to_string(),
                    failures: registration_error.into_iter().collect(),
                });
            }
            Ok(true) => self
                .backend
                .remove_leaf(&self.repo_root, &handle.path)
                .await
                .err(),
            Ok(false) => None,
            Err(error) => Some(error),
        };
        let branch_error = self
            .backend
            .delete_branch(&self.repo_root, &handle.branch)
            .await
            .err();
        if leaf_error.is_none() && branch_error.is_none() {
            return Ok(());
        }
        let failures = registration_error
            .into_iter()
            .chain(leaf_error)
            .chain(branch_error)
            .collect();
        Err(WorktreeError::CleanupFailed {
            context: handle.path.display().to_string(),
            failures,
        })
    }
}

fn safe_component(raw: &str) -> String {
    let mut value = raw
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    value.truncate(80);
    if value.trim_matches('-').is_empty() {
        "agent".to_string()
    } else {
        value
    }
}
