use std::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;

use crate::agent::worktree::{
    LocalWorktreeBackend, RemoteWorktreeBackend, WorktreeBackend, WorktreeHandle, WorktreeManager,
};
use crate::studio::agent_host::worktree_lease::{WorktreeLease, WorktreeLeaseState, load_leases};
use crate::studio::{
    StudioRecoveryIssue, StudioRecoveryIssueAction, StudioRecoveryIssueCategory,
    StudioRecoveryIssueScope,
};
use pl_tool::workspace::resolve_workspace_root;

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
            .map(|lease| normalized_local_path(Path::new(&lease.path)))
            .collect::<BTreeSet<_>>();
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
            let root = normalized_local_path(Path::new(String::from_utf8(root.stdout)?.trim()));
            let managed = root.join(".pure/worktrees");
            let mut found = BTreeSet::new();
            if let Ok(mut parents) = tokio::fs::read_dir(&managed).await {
                while let Some(parent) = parents.next_entry().await? {
                    if !parent.file_type().await?.is_dir() {
                        continue;
                    }
                    let mut children = tokio::fs::read_dir(parent.path()).await?;
                    while let Some(child) = children.next_entry().await? {
                        found.insert(normalized_local_path(&child.path()));
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
                    let path = normalized_local_path(Path::new(std::str::from_utf8(path)?));
                    if path.starts_with(&managed) {
                        found.insert(path);
                    }
                }
            }
            for path in found.difference(&known) {
                issues.push(StudioRecoveryIssue {
                    id: format!("unregistered-worktree:{}:{}", project.id, pl_core::context::content_hash(path.to_string_lossy().as_bytes())),
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
        manager.preview_existing(&handle).await?;
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
        use pl_core::thread::cold::ColdStore;
        for project in self.agent_facility.product_events.project_snapshot().await {
            for thread_id in self.store.list_project_thread_ids(&project.id).await? {
                let Some(thread) = self.store.read_thread_association(&thread_id).await? else {
                    continue;
                };
                let result = self.store.sessions().read_thread_journal(&thread.id).await;
                match result {
                    Ok(journal) => {
                        if let Some(commit) = pl_core::thread::journal::recovery_commit(&journal)? {
                            self.store.sessions().admit(
                                &thread.id,
                                commit.sequence,
                                commit.encode()?,
                            )?;
                        }
                    }
                    Err(error) => recovery_issues.push(StudioRecoveryIssue {
                        id: format!("session-context-{}", thread.id),
                        scope: StudioRecoveryIssueScope::Thread,
                        category: StudioRecoveryIssueCategory::AgentState,
                        action: StudioRecoveryIssueAction::CleanupThread,
                        project_id: Some(project.id.clone()),
                        thread_id: Some(thread.root_thread_id),
                        message: format!("Durable Thread {} is invalid: {error}", thread.id),
                        worktree: None,
                    }),
                }
            }
        }
        self.store.sessions().flush().await?;
        Ok(())
    }
}

#[cfg(windows)]
fn normalized_local_path(path: &Path) -> PathBuf {
    let path = dunce::simplified(path);
    PathBuf::from(path.to_string_lossy().replace('\\', "/"))
}

#[cfg(not(windows))]
fn normalized_local_path(path: &Path) -> PathBuf {
    dunce::simplified(path).to_path_buf()
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

#[cfg(test)]
mod tests {
    use super::*;
    use pl_core::{
        context::OpaquePayload,
        model::{DynModelSession, ModelError, ModelRequest, ModelSession, PreparedModelCall},
        thread::{ThreadHandle, cold::ColdStore, input::ThreadInput},
    };

    struct NoModel;
    impl ModelSession for NoModel {
        async fn prepare(&mut self, _: ModelRequest) -> Result<PreparedModelCall, ModelError> {
            panic!("recovery cannot execute models")
        }
        async fn close(&mut self) -> Result<(), ModelError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn corrupt_thread_produces_issue_without_blocking_other_thread_recovery() {
        let home = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let runtime = StudioRuntime::with_options(crate::StudioRuntimeOptions {
            studio_home: Some(home.path().into()),
            host: crate::StudioHostKind::Test,
        })
        .await
        .unwrap();
        runtime.start_runtime().await.unwrap();
        let project = runtime.open_project(workspace.path()).await.unwrap();
        runtime
            .persistence_repository()
            .await
            .unwrap()
            .flush()
            .await
            .unwrap();
        let bad = runtime
            .store
            .create_thread(&project.id, "bad", pl_protocol::ThreadModeId::simple())
            .await
            .unwrap();
        let good = runtime
            .store
            .create_thread(&project.id, "good", pl_protocol::ThreadModeId::simple())
            .await
            .unwrap();
        runtime
            .store
            .sessions()
            .admit(&bad.id, 1, OpaquePayload::text("corrupt journal envelope"))
            .unwrap();
        let handle = ThreadHandle::start(good.id.clone(), DynModelSession::new(NoModel)).unwrap();
        handle
            .submit_input(ThreadInput {
                id: "pending".into(),
                payload: OpaquePayload::text("must not execute"),
                context: Vec::new(),
            })
            .await
            .unwrap();
        let mut journal = handle.journal().await.unwrap();
        std::sync::Arc::make_mut(journal.last_mut().unwrap()).turn =
            Some(pl_core::thread::TurnRecord {
                turn_id: "unfinished".into(),
                input_id: None,
                state: pl_core::thread::TurnState::Running,
                model_steps: 0,
                elapsed_ms: None,
            });
        for commit in &journal {
            runtime
                .store
                .sessions()
                .admit(&good.id, commit.sequence, commit.encode().unwrap())
                .unwrap();
        }
        handle.close().await.unwrap();
        runtime.store.sessions().flush().await.unwrap();
        let mut issues = Vec::new();
        runtime
            .append_session_recovery_issues(&mut issues)
            .await
            .unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].thread_id.as_deref(), Some(bad.id.as_str()));
        assert_eq!(issues[0].action, StudioRecoveryIssueAction::CleanupThread);
        let restored = runtime
            .store
            .sessions()
            .replay_thread(&good.id)
            .await
            .unwrap();
        assert!(restored.commit_sequence > journal.last().unwrap().sequence);
        assert_eq!(
            restored.turns[0].state,
            pl_core::thread::TurnState::Interrupted
        );
        runtime.shutdown_runtime().await.unwrap();
    }
}
