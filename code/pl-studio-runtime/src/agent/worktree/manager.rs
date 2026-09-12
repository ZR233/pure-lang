use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{WorktreeBackend, WorktreeCreateFailureDisposition, WorktreeError, WorktreeStatus};

const WORKTREE_DIR: &str = ".anywork/worktrees";
const WORKTREE_BRANCH_PREFIX: &str = "pure-agent-";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeHandle {
    pub path: PathBuf,
    pub branch: String,
    pub base_commit: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeCreateSpec {
    pub repo_root: PathBuf,
    pub root_thread_id: String,
    pub child_id: String,
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

    pub fn allocate_path(repo_root: &Path, root_thread_id: &str, child_id: &str) -> PathBuf {
        repo_root
            .join(WORKTREE_DIR)
            .join(safe_component(root_thread_id))
            .join(safe_component(child_id))
    }

    pub fn branch_for(child_id: &str) -> String {
        format!("{WORKTREE_BRANCH_PREFIX}{}", safe_component(child_id))
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
        let path = Self::allocate_path(&self.repo_root, &spec.root_thread_id, &spec.child_id);
        let branch = Self::branch_for(&spec.child_id);
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
            || !handle.branch.starts_with(WORKTREE_BRANCH_PREFIX)
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

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;
    use crate::agent::worktree::LocalWorktreeBackend;

    fn git(cwd: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(cwd)
            .args(args)
            .output()
            .expect("git must launch");
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .expect("git output must be UTF-8")
            .trim()
            .to_string()
    }

    fn repository(name: &str) -> tempfile::TempDir {
        let repository = tempfile::Builder::new()
            .prefix(&format!("pure-worktree-{name}-"))
            .tempdir()
            .unwrap();
        git(repository.path(), &["init"]);
        git(
            repository.path(),
            &["config", "user.email", "pure@example.invalid"],
        );
        git(repository.path(), &["config", "user.name", "Pure Test"]);
        std::fs::write(repository.path().join("tracked.txt"), "base\n").unwrap();
        git(repository.path(), &["add", "tracked.txt"]);
        git(repository.path(), &["commit", "-m", "initial"]);
        repository
    }

    #[tokio::test]
    async fn local_worktree_does_not_copy_dirty_main_workspace_and_cleans_explicitly() {
        let repository = repository("local-lifecycle");
        let root = std::fs::canonicalize(repository.path()).unwrap();
        #[cfg(windows)]
        assert!(
            root.to_string_lossy().starts_with(r"\\?\"),
            "the regression fixture must exercise a verbatim Windows path"
        );
        let manager = WorktreeManager::new(root.clone(), Arc::new(LocalWorktreeBackend::default()));
        let base = manager.resolve_head(&root).await.unwrap();
        std::fs::write(root.join("tracked.txt"), "dirty main\n").unwrap();

        let handle = manager
            .create(WorktreeCreateSpec {
                repo_root: root.clone(),
                root_thread_id: "root-thread".to_string(),
                child_id: "child-agent".to_string(),
                base_commit: base.clone(),
            })
            .await
            .unwrap();

        assert_eq!(
            std::fs::read_to_string(handle.path.join("tracked.txt"))
                .unwrap()
                .trim_end_matches(['\r', '\n']),
            "base"
        );
        std::fs::write(handle.path.join("child.txt"), "child\n").unwrap();
        let dirty = manager.preview(&handle).await.unwrap();
        assert_eq!(dirty.head, base);
        assert_eq!(dirty.changed_files, ["child.txt"]);
        git(&handle.path, &["add", "child.txt"]);
        git(&handle.path, &["commit", "-m", "child change"]);
        let committed = manager.preview(&handle).await.unwrap();
        assert_ne!(committed.head, base);
        assert!(committed.changed_files.is_empty());
        assert!(!root.join("child.txt").exists());

        manager.discard(&handle).await.unwrap();

        assert!(!handle.path.exists());
        assert!(
            git(&root, &["branch", "--list", &handle.branch]).is_empty(),
            "Pure-owned branch must be removed"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("tracked.txt")).unwrap(),
            "dirty main\n"
        );
    }

    #[derive(Debug)]
    struct UnreadableCreatedHead {
        local: LocalWorktreeBackend,
        refuse_cleanup: bool,
    }

    impl WorktreeBackend for UnreadableCreatedHead {
        fn resolve_repo_root<'a>(
            &'a self,
            path: &'a Path,
        ) -> futures::future::BoxFuture<'a, Result<PathBuf, WorktreeError>> {
            self.local.resolve_repo_root(path)
        }
        fn create_parent<'a>(
            &'a self,
            root: &'a Path,
            path: &'a Path,
        ) -> futures::future::BoxFuture<'a, Result<(), WorktreeError>> {
            self.local.create_parent(root, path)
        }
        fn path_exists<'a>(
            &'a self,
            path: &'a Path,
        ) -> futures::future::BoxFuture<'a, Result<bool, WorktreeError>> {
            self.local.path_exists(path)
        }
        fn remove_leaf<'a>(
            &'a self,
            root: &'a Path,
            path: &'a Path,
        ) -> futures::future::BoxFuture<'a, Result<(), WorktreeError>> {
            self.local.remove_leaf(root, path)
        }
        fn create<'a>(
            &'a self,
            root: &'a Path,
            branch: &'a str,
            path: &'a Path,
            base: &'a str,
        ) -> futures::future::BoxFuture<'a, Result<(), super::super::WorktreeCreateFailure>>
        {
            self.local.create(root, branch, path, base)
        }
        fn resolve_head<'a>(
            &'a self,
            _path: &'a Path,
        ) -> futures::future::BoxFuture<'a, Result<String, WorktreeError>> {
            Box::pin(async {
                Err(WorktreeError::GitStatusUnknown {
                    args: "rev-parse HEAD".into(),
                    stderr: "injected verification read failure".into(),
                })
            })
        }
        fn status<'a>(
            &'a self,
            path: &'a Path,
        ) -> futures::future::BoxFuture<'a, Result<WorktreeStatus, WorktreeError>> {
            self.local.status(path)
        }
        fn remove<'a>(
            &'a self,
            root: &'a Path,
            path: &'a Path,
            force: bool,
        ) -> futures::future::BoxFuture<'a, Result<(), WorktreeError>> {
            if self.refuse_cleanup {
                Box::pin(async {
                    Err(WorktreeError::InvalidResource(
                        "injected cleanup refusal".into(),
                    ))
                })
            } else {
                self.local.remove(root, path, force)
            }
        }
        fn delete_branch<'a>(
            &'a self,
            root: &'a Path,
            branch: &'a str,
        ) -> futures::future::BoxFuture<'a, Result<(), WorktreeError>> {
            self.local.delete_branch(root, branch)
        }
    }

    #[tokio::test]
    async fn head_read_failure_after_creation_rolls_back_or_reports_retained_resources() {
        for refuse_cleanup in [false, true] {
            let repository = repository("verification-failure");
            let root = std::fs::canonicalize(repository.path()).unwrap();
            let base = git(&root, &["rev-parse", "HEAD"]);
            let manager = WorktreeManager::new(
                root.clone(),
                Arc::new(UnreadableCreatedHead {
                    local: LocalWorktreeBackend::default(),
                    refuse_cleanup,
                }),
            );
            let handle = WorktreeHandle {
                path: WorktreeManager::allocate_path(&root, "root", "child"),
                branch: WorktreeManager::branch_for("child"),
                base_commit: base.clone(),
            };
            let failure = manager
                .create(WorktreeCreateSpec {
                    repo_root: root.clone(),
                    root_thread_id: "root".into(),
                    child_id: "child".into(),
                    base_commit: base,
                })
                .await
                .unwrap_err();
            if refuse_cleanup {
                assert!(
                    matches!(failure, WorktreeError::OperationFailedWithCleanup { .. }),
                    "{failure:?}"
                );
                assert!(
                    handle.path.exists(),
                    "a refused cleanup must remain owned for retry"
                );
                WorktreeManager::new(root.clone(), Arc::new(LocalWorktreeBackend::default()))
                    .discard(&handle)
                    .await
                    .unwrap();
            } else {
                assert!(
                    matches!(failure, WorktreeError::OperationFailedAfterCleanup { .. }),
                    "{failure:?}"
                );
                assert!(!handle.path.exists());
            }
            assert!(git(&root, &["branch", "--list", &handle.branch]).is_empty());
        }
    }

    #[tokio::test]
    async fn cleanup_refuses_non_pure_or_non_leaf_identity() {
        let repository = repository("unsafe-cleanup");
        let root = std::fs::canonicalize(repository.path()).unwrap();
        let manager = WorktreeManager::new(root.clone(), Arc::new(LocalWorktreeBackend::default()));
        let handle = WorktreeHandle {
            path: root.join("outside"),
            branch: "main".to_string(),
            base_commit: git(&root, &["rev-parse", "HEAD"]),
        };

        let error = manager.discard(&handle).await.unwrap_err().to_string();

        assert!(error.contains("non-Pure or non-leaf"), "{error}");
    }
}
