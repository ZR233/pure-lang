//! Product workspace allocation independent of Agent execution and model sessions.
use super::worktree_lease::{
    WorktreeLease, WorktreeLeaseOwner, WorktreeLeaseOwnerKind, WorktreeLeaseState,
};
use crate::agent::worktree::{
    LocalWorktreeBackend, RemoteWorktreeBackend, WorktreeBackend, WorktreeCreateSpec,
    WorktreeManager, WorktreeOwnership,
};
use crate::studio::records::ProjectRecord;
use crate::{PureError, Result};
use pl_protocol::{AgentWorkspaceAssignmentSnapshot, AgentWorkspaceMode, AgentWorktreeSnapshot};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

pub(in crate::studio) struct WorkspacePreparation<'a> {
    pub project: &'a ProjectRecord,
    pub root_thread_id: &'a str,
    pub child_id: &'a str,
    pub mode: AgentWorkspaceMode,
    pub writable_paths: Option<Vec<String>>,
    /// 根会话的工作区根；`local` 会话等于 canonical Project 路径，`worktree` 会话为
    /// 其自身 worktree。非 worktree child 的工作区根跟随该值。
    pub session_root: PathBuf,
}

pub(in crate::studio) struct SpawnWorktreeLease {
    pub lease: WorktreeLease,
}

pub(in crate::studio) async fn prepare_workspace(
    worktrees: &WorktreeLeaseOwner,
    ssh_manager: &Arc<pl_tool::remote::SshManager>,
    request: WorkspacePreparation<'_>,
) -> Result<(AgentWorkspaceAssignmentSnapshot, Option<SpawnWorktreeLease>)> {
    let project_root = resolved_project_root(request.project)?;
    let session_root = request.session_root;
    let child_id = request.child_id.to_owned();
    let mode = request.mode;
    if mode != AgentWorkspaceMode::Directory && request.writable_paths.is_some() {
        return Err(lifecycle_error(
            "only directory Profiles accept writablePaths",
        ));
    }
    let writable_paths = normalize_scope(request.writable_paths)?;
    let project_root_text = project_root.to_string_lossy().into_owned();
    let session_root_text = session_root.to_string_lossy().into_owned();

    let (assignment, worktree) = match mode {
        AgentWorkspaceMode::Unrestricted => (
            AgentWorkspaceAssignmentSnapshot {
                mode,
                project_root: project_root_text,
                root: session_root_text,
                writable_paths: None,
                worktree: None,
            },
            None,
        ),
        AgentWorkspaceMode::Directory => {
            let writable_paths = writable_paths.map(|paths| {
                paths
                    .into_iter()
                    .map(|path| {
                        if path == "." {
                            session_root.clone()
                        } else {
                            session_root.join(path)
                        }
                        .to_string_lossy()
                        .into_owned()
                    })
                    .collect()
            });
            (
                AgentWorkspaceAssignmentSnapshot {
                    mode,
                    project_root: project_root_text,
                    root: session_root_text,
                    writable_paths,
                    worktree: None,
                },
                None,
            )
        }
        AgentWorkspaceMode::Worktree => {
            let backend = backend_for(
                ssh_manager,
                request.project.ssh_alias.as_deref(),
                &project_root,
            );
            let repository_root =
                WorktreeManager::resolve_repository_root(backend.as_ref(), &project_root)
                    .await
                    .map_err(|error| lifecycle_error(error.to_string()))?;
            let manager = WorktreeManager::new(repository_root.clone(), backend);
            let base_commit = manager
                .resolve_head(&repository_root)
                .await
                .map_err(|error| lifecycle_error(error.to_string()))?;
            let ownership = WorktreeOwnership::Child {
                child_id: child_id.clone(),
            };
            let path = WorktreeManager::allocate_path(
                &repository_root,
                request.root_thread_id,
                &ownership,
            );
            let branch = WorktreeManager::branch_for(&ownership);
            let mut durable = WorktreeLease {
                revision: 1,
                state: WorktreeLeaseState::Prepared,
                owner_kind: WorktreeLeaseOwnerKind::Child,
                owner_thread_id: child_id.clone(),
                root_thread_id: request.root_thread_id.to_owned(),
                project_id: request.project.id.clone(),
                ssh_alias: request.project.ssh_alias.clone(),
                repository_root: repository_root.to_string_lossy().into_owned(),
                path: path.to_string_lossy().into_owned(),
                branch: branch.clone(),
                base_commit: base_commit.clone(),
            };
            worktrees
                .record(durable.clone())
                .map_err(|error| lifecycle_error(error.to_string()))?;
            let handle = match manager
                .create(WorktreeCreateSpec {
                    repo_root: repository_root.clone(),
                    root_thread_id: request.root_thread_id.to_owned(),
                    ownership,
                    base_commit: base_commit.clone(),
                })
                .await
            {
                Ok(handle) => handle,
                Err(error) => {
                    settle_failed_create(worktrees, durable, &error).await;
                    return Err(lifecycle_error(error.to_string()));
                }
            };
            durable.path = handle.path.to_string_lossy().into_owned();
            durable.branch = handle.branch.clone();
            (
                AgentWorkspaceAssignmentSnapshot {
                    mode,
                    project_root: project_root.to_string_lossy().into_owned(),
                    root: handle.path.to_string_lossy().into_owned(),
                    writable_paths: None,
                    worktree: Some(AgentWorktreeSnapshot {
                        repository_root: repository_root.to_string_lossy().into_owned(),
                        path: handle.path.to_string_lossy().into_owned(),
                        branch: handle.branch.clone(),
                        base_commit,
                    }),
                },
                Some(SpawnWorktreeLease { lease: durable }),
            )
        }
    };

    Ok((assignment, worktree))
}

pub(in crate::studio) fn backend_for(
    ssh_manager: &Arc<pl_tool::remote::SshManager>,
    ssh_alias: Option<&str>,
    project_root: &Path,
) -> Arc<dyn WorktreeBackend> {
    match ssh_alias {
        Some(server_id) => Arc::new(RemoteWorktreeBackend::new(
            ssh_manager.clone(),
            server_id,
            project_root.to_path_buf(),
        )),
        None => Arc::new(LocalWorktreeBackend::default()),
    }
}

pub(in crate::studio) fn manager_from_lease(
    ssh_manager: &Arc<pl_tool::remote::SshManager>,
    lease: &WorktreeLease,
) -> WorktreeManager {
    let repository_root = PathBuf::from(&lease.repository_root);
    WorktreeManager::new(
        repository_root.clone(),
        backend_for(ssh_manager, lease.ssh_alias.as_deref(), &repository_root),
    )
}

/// Creates the physical worktree of a root session and records its `prepared` lease.
///
/// Preflight (local Project, repository root and `HEAD` resolution) runs before any
/// resource exists. A failed physical create settles the durable lease through the
/// existing `WorktreeCreateFailureDisposition` and never bypasses a Git lock or a
/// registered worktree identity.
pub(in crate::studio) async fn create_root_session_worktree(
    worktrees: &WorktreeLeaseOwner,
    ssh_manager: &Arc<pl_tool::remote::SshManager>,
    project: &ProjectRecord,
    thread_id: &str,
) -> Result<WorktreeLease> {
    if project.ssh_alias.is_some() {
        return Err(lifecycle_error(format!(
            "worktree sessions are only available for local projects; Project {} is remote",
            project.id
        )));
    }
    let project_root = resolved_project_root(project)?;
    let backend = backend_for(ssh_manager, None, &project_root);
    let repository_root = WorktreeManager::resolve_repository_root(backend.as_ref(), &project_root)
        .await
        .map_err(|error| lifecycle_error(error.to_string()))?;
    let manager = WorktreeManager::new(repository_root.clone(), backend);
    let base_commit = manager
        .resolve_head(&repository_root)
        .await
        .map_err(|error| lifecycle_error(error.to_string()))?;
    let ownership = WorktreeOwnership::Session {
        thread_id: thread_id.to_owned(),
    };
    let path = WorktreeManager::allocate_path(&repository_root, thread_id, &ownership);
    let branch = WorktreeManager::branch_for(&ownership);
    let durable = WorktreeLease {
        revision: 1,
        state: WorktreeLeaseState::Prepared,
        owner_kind: WorktreeLeaseOwnerKind::Session,
        owner_thread_id: thread_id.to_owned(),
        root_thread_id: thread_id.to_owned(),
        project_id: project.id.clone(),
        ssh_alias: None,
        repository_root: repository_root.to_string_lossy().into_owned(),
        path: path.to_string_lossy().into_owned(),
        branch,
        base_commit: base_commit.clone(),
    };
    worktrees
        .record(durable.clone())
        .map_err(|error| lifecycle_error(error.to_string()))?;
    let handle = match manager
        .create(WorktreeCreateSpec {
            repo_root: repository_root,
            root_thread_id: thread_id.to_owned(),
            ownership,
            base_commit,
        })
        .await
    {
        Ok(handle) => handle,
        Err(error) => {
            settle_failed_create(worktrees, durable, &error).await;
            return Err(lifecycle_error(error.to_string()));
        }
    };
    if handle.path != path || handle.branch != durable.branch {
        return Err(settle_identity_mismatch(worktrees, durable));
    }
    Ok(durable)
}

/// 收束一个身份不符的已创建会话 worktree：保留现场（`preserved`）并返回类型化错误。
///
/// 绝不删除物理资源，也不留下停在 `prepared` 的 lease；调用方负责随后的发布。
fn settle_identity_mismatch(worktrees: &WorktreeLeaseOwner, mut lease: WorktreeLease) -> PureError {
    let error =
        lifecycle_error("created session worktree identity differs from its Pure-owned lease");
    lease.transition(WorktreeLeaseState::Preserved);
    if let Err(persist_error) = worktrees.record(lease.clone()) {
        tracing::error!(
            owner_thread_id = lease.owner_thread_id,
            error = %persist_error,
            "failed to preserve an identity-mismatched session worktree lease"
        );
    }
    error
}

/// Resolves the workspace root of a root session from its canonical product fact.
///
/// `local` uses the canonical Project root. `worktree` requires an identity-matching
/// `active` session lease; a missing, mismatched or cleaned lease fails explicitly and
/// never silently falls back to the main workspace.
pub(in crate::studio) fn root_session_workspace_root(
    worktrees: &WorktreeLeaseOwner,
    workspace_mode: pl_protocol::ThreadWorkspaceMode,
    root_thread_id: &str,
    project: &ProjectRecord,
) -> Result<PathBuf> {
    use pl_protocol::ThreadWorkspaceMode;
    let project_root = resolved_project_root(project)?;
    match workspace_mode {
        ThreadWorkspaceMode::Local => Ok(project_root),
        ThreadWorkspaceMode::Worktree => {
            let lease = worktrees.get(root_thread_id).ok_or_else(|| {
                lifecycle_error(format!(
                    "Thread {root_thread_id} workspace mode is worktree but no durable lease exists"
                ))
            })?;
            if lease.owner_kind != WorktreeLeaseOwnerKind::Session {
                return Err(lifecycle_error(format!(
                    "Thread {root_thread_id} workspace lease is not owned by its session"
                )));
            }
            if lease.state != WorktreeLeaseState::Active {
                return Err(lifecycle_error(format!(
                    "Thread {root_thread_id} session worktree lease is {} instead of active",
                    lease.state.label()
                )));
            }
            lease
                .validate_identity()
                .map_err(|error| lifecycle_error(error.to_string()))?;
            Ok(PathBuf::from(&lease.path))
        }
    }
}

async fn settle_failed_create(
    worktrees: &WorktreeLeaseOwner,
    mut lease: WorktreeLease,
    error: &crate::agent::worktree::WorktreeError,
) {
    let state = match error {
        crate::agent::worktree::WorktreeError::OperationFailedWithCleanup { .. }
        | crate::agent::worktree::WorktreeError::CleanupFailed { .. } => {
            WorktreeLeaseState::Preserved
        }
        _ => WorktreeLeaseState::Cleaned,
    };
    lease.transition(state);
    if let Err(persist_error) = worktrees.record(lease.clone()) {
        tracing::error!(
            owner_thread_id = lease.owner_thread_id,
            error = %persist_error,
            "failed to settle durable worktree lease after create failure"
        );
    }
}

pub(in crate::studio) fn resolved_project_root(project: &ProjectRecord) -> Result<PathBuf> {
    if project.ssh_alias.is_some() {
        let value = project.path.trim().replace('\\', "/");
        if value.is_empty() || value == "/" || value.split('/').any(|part| part == "..") {
            return Err(lifecycle_error(format!(
                "invalid remote project workspace: {}",
                project.path
            )));
        }
        Ok(PathBuf::from(value))
    } else {
        pl_tool::workspace::resolve_workspace_root(&PathBuf::from(&project.path))
            .map_err(|error| lifecycle_error(error.to_string()))
    }
}

fn lifecycle_error(error: impl Into<String>) -> PureError {
    PureError::ToolExecutionFailed {
        tool: "studio_workspace".into(),
        error: error.into(),
    }
}

fn normalize_scope(requested: Option<Vec<String>>) -> Result<Option<Vec<String>>> {
    let Some(requested) = requested else {
        return Ok(None);
    };
    let mut normalized = std::collections::BTreeSet::new();
    for path in requested {
        let value = path.trim();
        if value.is_empty()
            || value.contains(['\\', '\0', ':'])
            || value.starts_with('/')
            || value.contains("//")
        {
            return Err(lifecycle_error(format!(
                "invalid project-relative write directory: {path}"
            )));
        }
        let mut parts = Vec::new();
        for component in value.split('/') {
            match component {
                ".." => {
                    return Err(lifecycle_error(format!(
                        "write directory escapes the project: {path}"
                    )));
                }
                "." | "" => {}
                part => parts.push(part),
            }
        }
        normalized.insert(if parts.is_empty() {
            ".".to_owned()
        } else {
            parts.join("/")
        });
    }
    if normalized.contains(".") {
        return Ok(Some(vec![".".to_owned()]));
    }
    let mut compact: Vec<String> = Vec::new();
    for candidate in normalized {
        if !compact.iter().any(|parent| {
            candidate
                .strip_prefix(parent)
                .is_some_and(|tail| tail.starts_with('/'))
        }) {
            compact.push(candidate);
        }
    }
    Ok(Some(compact))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    use crate::agent::worktree::WorktreeError;
    use crate::studio::agent_host::worktree_lease::WorktreeLeaseOwnerKind;

    async fn lease_owner() -> WorktreeLeaseOwner {
        let store = crate::studio::StudioStore::open_memory().await.unwrap();
        WorktreeLeaseOwner::new(crate::studio::agent_host::ThreadWriteBehindWriter::new(
            store,
        ))
    }

    fn session_lease(thread_id: &str) -> WorktreeLease {
        WorktreeLease {
            revision: 1,
            state: WorktreeLeaseState::Prepared,
            owner_kind: WorktreeLeaseOwnerKind::Session,
            owner_thread_id: thread_id.to_string(),
            root_thread_id: thread_id.to_string(),
            project_id: "project-1".into(),
            ssh_alias: None,
            repository_root: "/repo".into(),
            path: format!("/repo/.anywork/worktrees/{thread_id}/session"),
            branch: format!("pure-session-{thread_id}"),
            base_commit: "base".into(),
        }
    }

    /// F3：创建阶段失败收束绝不停留在 `prepared`；不确定现场一律保留为 `preserved`，
    /// 只有确认无副作用时才收束为 `cleaned`。
    #[tokio::test]
    async fn creation_failure_settles_to_preserved_when_the_scene_must_be_kept() {
        let worktrees = lease_owner().await;
        for error in [
            WorktreeError::OperationFailedWithCleanup {
                operation: Box::new(WorktreeError::InvalidResource("create".into())),
                cleanup: Box::new(WorktreeError::CleanupFailed {
                    context: "/repo".into(),
                    failures: Vec::new(),
                }),
            },
            WorktreeError::CleanupFailed {
                context: "/repo".into(),
                failures: Vec::new(),
            },
        ] {
            let thread_id = crate::studio::ids::new_id("thread");
            let lease = session_lease(&thread_id);
            worktrees.record(lease.clone()).unwrap();
            settle_failed_create(&worktrees, lease, &error).await;
            let settled = worktrees.get(&thread_id).unwrap();
            assert_eq!(settled.state, WorktreeLeaseState::Preserved);
            assert_eq!(settled.revision, 2);
        }

        let thread_id = crate::studio::ids::new_id("thread");
        let lease = session_lease(&thread_id);
        worktrees.record(lease.clone()).unwrap();
        settle_failed_create(
            &worktrees,
            lease,
            &WorktreeError::InvalidResource("preflight".into()),
        )
        .await;
        assert_eq!(
            worktrees.get(&thread_id).unwrap().state,
            WorktreeLeaseState::Cleaned
        );
    }

    /// F3：创建过程身份不符时必须收束为 `preserved` 并保留现场，不能留下 `prepared`。
    #[tokio::test]
    async fn identity_mismatch_after_creation_preserves_the_scene() {
        let worktrees = lease_owner().await;
        let thread_id = crate::studio::ids::new_id("thread");
        let lease = session_lease(&thread_id);
        worktrees.record(lease.clone()).unwrap();
        let error = settle_identity_mismatch(&worktrees, lease);
        assert!(error.to_string().contains("identity differs"), "{error}");
        let settled = worktrees.get(&thread_id).unwrap();
        assert_eq!(settled.state, WorktreeLeaseState::Preserved);
        assert_eq!(settled.revision, 2);
        assert!(settled.validate_identity().is_ok());
    }

    #[test]
    fn directory_scope_preserves_readonly_and_rejects_cross_platform_escape() {
        assert_eq!(normalize_scope(None).unwrap(), None);
        assert_eq!(normalize_scope(Some(Vec::new())).unwrap(), Some(Vec::new()));
        assert_eq!(
            normalize_scope(Some(vec![
                "src/nested".into(),
                "./src".into(),
                "other/".into()
            ]))
            .unwrap(),
            Some(vec!["other".into(), "src".into()])
        );
        for path in ["../outside", "/outside", "C:/outside", "a\\b", "a/../b", ""] {
            assert!(normalize_scope(Some(vec![path.into()])).is_err(), "{path}");
        }
    }
}
