use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;

use crate::agent::worktree::{
    LocalWorktreeBackend, RemoteWorktreeBackend, WorktreeBackend, WorktreeHandle, WorktreeManager,
};
use crate::resolve_workspace_root;
use crate::studio::agent_host::worktree_lease::{WorktreeLease, WorktreeLeaseState, load_leases};
use crate::studio::{
    StudioRecoveryIssue, StudioRecoveryIssueAction, StudioRecoveryIssueCategory,
    StudioRecoveryIssueScope,
};

use super::super::StudioRuntime;

impl StudioRuntime {
    pub(in crate::studio::runtime) async fn append_worktree_recovery_issues(
        &self,
        recovery_issues: &mut Vec<StudioRecoveryIssue>,
    ) -> Result<()> {
        self.agent_facility
            .worktrees
            .restore(load_leases(&self.store).await?);
        for lease in self.agent_facility.worktrees.snapshot() {
            if lease.state == WorktreeLeaseState::Cleaned {
                continue;
            }
            recovery_issues.push(self.worktree_recovery_issue(&lease).await);
        }
        self.append_unregistered_worktrees(recovery_issues).await?;
        Ok(())
    }

    /// Discover local resources whose last lease may have been lost with an unflushed process.
    /// Unknown ownership is diagnostic only: no inferred lease or automatic deletion.
    async fn append_unregistered_worktrees(
        &self,
        issues: &mut Vec<StudioRecoveryIssue>,
    ) -> Result<()> {
        let known = self
            .agent_facility
            .worktrees
            .snapshot()
            .into_iter()
            .filter(|lease| lease.state != WorktreeLeaseState::Cleaned)
            .map(|lease| PathBuf::from(lease.path))
            .collect::<std::collections::BTreeSet<_>>();
        for project in self.agent_facility.product_events.project_snapshot().await {
            if project.ssh_server_id.is_some() {
                continue;
            }
            let project_path = PathBuf::from(&project.path);
            let root = tokio::process::Command::new("git")
                .args(["rev-parse", "--show-toplevel"])
                .current_dir(&project_path)
                .output()
                .await;
            let Ok(root) = root else {
                continue;
            };
            if !root.status.success() {
                continue;
            }
            let root = PathBuf::from(String::from_utf8(root.stdout)?.trim());
            let managed = root.join(".pure/worktrees");
            let mut found = std::collections::BTreeSet::new();
            if let Ok(mut parents) = tokio::fs::read_dir(&managed).await {
                while let Some(parent) = parents.next_entry().await? {
                    if !parent.file_type().await?.is_dir() {
                        continue;
                    }
                    let mut children = tokio::fs::read_dir(parent.path()).await?;
                    while let Some(child) = children.next_entry().await? {
                        found.insert(child.path());
                    }
                }
            }
            let registered = tokio::process::Command::new("git")
                .args(["worktree", "list", "--porcelain", "-z"])
                .current_dir(&root)
                .output()
                .await?;
            anyhow::ensure!(
                registered.status.success(),
                "cannot inspect registered worktrees for {}: {}",
                project.id,
                String::from_utf8_lossy(&registered.stderr)
            );
            for field in registered.stdout.split(|byte| *byte == 0) {
                if let Some(path) = field.strip_prefix(b"worktree ") {
                    let path = PathBuf::from(std::str::from_utf8(path)?);
                    if path.starts_with(&managed) {
                        found.insert(path);
                    }
                }
            }
            for path in found.difference(&known) {
                issues.push(StudioRecoveryIssue {
                    id: format!("unregistered-worktree:{}:{}", project.id, pl_core::canonical_content_hash(path.to_string_lossy().as_bytes())),
                    scope: StudioRecoveryIssueScope::Project, category: StudioRecoveryIssueCategory::Repository,
                    action: StudioRecoveryIssueAction::Retry, project_id: Some(project.id.clone()), thread_id: None,
                    message: format!("Unregistered worktree preserved at {}; ownership must be inspected before explicit cleanup", path.display()), worktree: None,
                });
            }
        }
        Ok(())
    }

    async fn worktree_recovery_issue(&self, lease: &WorktreeLease) -> StudioRecoveryIssue {
        let manager = self.worktree_manager(lease);
        let handle = worktree_handle(lease);
        let identity_error = validate_lease_identity(lease).err();
        let preview = match identity_error.as_ref() {
            Some(_) => None,
            None => manager.preview(&handle).await.ok(),
        };
        let diagnostic = identity_error.map(|error| error.to_string()).or_else(|| {
            preview
                .is_none()
                .then(|| "worktree preview failed; the resource was preserved".to_string())
        });
        let changed_files = preview
            .as_ref()
            .map(|preview| preview.changed_files.clone())
            .unwrap_or_default();
        StudioRecoveryIssue {
            id: worktree_issue_id(&lease.child_id),
            scope: StudioRecoveryIssueScope::Thread,
            category: StudioRecoveryIssueCategory::Repository,
            action: StudioRecoveryIssueAction::CleanupWorktree,
            project_id: Some(lease.project_id.clone()),
            thread_id: Some(lease.root_thread_id.clone()),
            message: diagnostic.unwrap_or_else(|| {
                format!(
                    "Agent worktree {} is preserved for explicit review and cleanup",
                    lease.branch
                )
            }),
            worktree: Some(crate::StudioWorktreeRecoveryPreview {
                child_id: lease.child_id.clone(),
                lease_revision: lease.revision,
                state: lease.state.label().to_string(),
                repository_root: lease.repository_root.clone(),
                path: lease.path.clone(),
                branch: lease.branch.clone(),
                base_commit: lease.base_commit.clone(),
                head_commit: preview.as_ref().map(|preview| preview.head.clone()),
                dirty: !changed_files.is_empty(),
                changed_files,
            }),
        }
    }

    pub async fn cleanup_preserved_worktree(
        &self,
        child_id: &str,
        expected_lease_revision: u64,
    ) -> Result<()> {
        let mut lease = self
            .agent_facility
            .worktrees
            .get(child_id)
            .ok_or_else(|| anyhow::anyhow!("worktree lease does not exist"))?;
        anyhow::ensure!(
            lease.revision == expected_lease_revision,
            "worktree lease revision conflict: expected {expected_lease_revision}, actual {}",
            lease.revision
        );
        anyhow::ensure!(
            lease.state != WorktreeLeaseState::Cleaned
                && lease.state != WorktreeLeaseState::CleanupRequested,
            "worktree lease is already cleaned"
        );
        validate_lease_identity(&lease)?;
        let manager = self.worktree_manager(&lease);
        let handle = worktree_handle(&lease);
        manager.preview(&handle).await?;
        lease.transition(WorktreeLeaseState::CleanupRequested);
        self.agent_facility.worktrees.record(lease.clone())?;
        if let Err(error) = manager.discard(&handle).await {
            lease.transition(WorktreeLeaseState::Preserved);
            self.agent_facility.worktrees.record(lease.clone())?;
            return Err(error.into());
        }
        lease.transition(WorktreeLeaseState::Cleaned);
        self.agent_facility.worktrees.record(lease.clone())?;
        let issues = self.recovery.remove(&worktree_issue_id(child_id));
        self.agent_facility
            .product_events
            .emit_recovery_state(issues);
        Ok(())
    }

    fn worktree_manager(&self, lease: &WorktreeLease) -> WorktreeManager {
        let repository_root = PathBuf::from(&lease.repository_root);
        let backend: Arc<dyn WorktreeBackend> = match lease.ssh_server_id.as_deref() {
            Some(server_id) => Arc::new(RemoteWorktreeBackend::new(
                self.ssh_manager.clone(),
                server_id,
                repository_root.clone(),
            )),
            None => Arc::new(LocalWorktreeBackend::default()),
        };
        WorktreeManager::new(repository_root, backend)
    }

    pub(in crate::studio::runtime) async fn append_unavailable_project_recovery_issues(
        &self,
        recovery_issues: &mut Vec<StudioRecoveryIssue>,
    ) -> Result<()> {
        for project in self.store.list_projects().await? {
            if project.ssh_server_id.is_some() {
                continue;
            }
            let Err(error) = resolve_workspace_root(Path::new(&project.path)) else {
                continue;
            };
            if recovery_issues.iter().any(|issue| {
                issue.scope == StudioRecoveryIssueScope::Project
                    && issue.project_id.as_deref() == Some(project.id.as_str())
            }) {
                continue;
            }
            recovery_issues.push(StudioRecoveryIssue {
                id: format!("recovery-issue-project-path-{}", project.id),
                scope: StudioRecoveryIssueScope::Project,
                category: StudioRecoveryIssueCategory::Repository,
                action: StudioRecoveryIssueAction::RemoveProject,
                project_id: Some(project.id),
                thread_id: None,
                message: format!("Project workspace is unavailable: {error}"),
                worktree: None,
            });
        }
        Ok(())
    }

    pub(super) async fn append_session_recovery_issues(
        &self,
        recovery_issues: &mut Vec<StudioRecoveryIssue>,
    ) -> Result<()> {
        let Some(repository) = self.persistence_repository().await else {
            return Ok(());
        };
        let failures = repository.audit_registered_sessions().await?;
        let mut failures_by_root = BTreeMap::<(String, String), Vec<_>>::new();
        for failure in failures {
            failures_by_root
                .entry((failure.project_id.clone(), failure.root_thread_id.clone()))
                .or_default()
                .push(failure);
        }
        for ((project_id, root_thread_id), failures) in failures_by_root {
            let affected = failures
                .iter()
                .map(|failure| failure.agent_thread_id.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            let detail = failures
                .first()
                .map(|failure| failure.detail.as_str())
                .unwrap_or("invalid durable session snapshot");
            recovery_issues.push(StudioRecoveryIssue {
                id: format!("session-context-{root_thread_id}"),
                scope: StudioRecoveryIssueScope::Thread,
                category: StudioRecoveryIssueCategory::AgentState,
                action: StudioRecoveryIssueAction::CleanupThread,
                project_id: Some(project_id),
                thread_id: Some(root_thread_id),
                message: format!(
                    "Durable Agent session context is invalid for {affected}: {detail}"
                ),
                worktree: None,
            });
        }
        Ok(())
    }
}

fn worktree_issue_id(child_id: &str) -> String {
    format!("worktree-lease-{child_id}")
}

fn worktree_handle(lease: &WorktreeLease) -> WorktreeHandle {
    WorktreeHandle {
        path: PathBuf::from(&lease.path),
        branch: lease.branch.clone(),
        base_commit: lease.base_commit.clone(),
    }
}

fn validate_lease_identity(lease: &WorktreeLease) -> Result<()> {
    let repository_root = PathBuf::from(&lease.repository_root);
    let expected_path =
        WorktreeManager::allocate_path(&repository_root, &lease.root_thread_id, &lease.child_id);
    anyhow::ensure!(
        Path::new(&lease.path) == expected_path,
        "worktree cleanup refused a mismatched Pure-owned leaf"
    );
    anyhow::ensure!(
        lease.branch == WorktreeManager::branch_for(&lease.child_id),
        "worktree cleanup refused a mismatched Pure-owned branch"
    );
    Ok(())
}
