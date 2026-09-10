//! Product workspace allocation independent of Agent execution and model sessions.
use super::worktree_lease::{WorktreeLease, WorktreeLeaseOwner, WorktreeLeaseState};
use crate::agent::worktree::{
    LocalWorktreeBackend, RemoteWorktreeBackend, WorktreeBackend, WorktreeCreateSpec,
    WorktreeManager,
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
    let child_id = request.child_id.to_owned();
    let mode = request.mode;
    if mode != AgentWorkspaceMode::Directory && request.writable_paths.is_some() {
        return Err(lifecycle_error(
            "only directory Profiles accept writablePaths",
        ));
    }
    let writable_paths = normalize_scope(request.writable_paths)?;

    let (assignment, worktree) = match mode {
        AgentWorkspaceMode::Unrestricted => (
            AgentWorkspaceAssignmentSnapshot {
                mode,
                project_root: project_root.to_string_lossy().into_owned(),
                root: project_root.to_string_lossy().into_owned(),
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
                            project_root.clone()
                        } else {
                            project_root.join(path)
                        }
                        .to_string_lossy()
                        .into_owned()
                    })
                    .collect()
            });
            (
                AgentWorkspaceAssignmentSnapshot {
                    mode,
                    project_root: project_root.to_string_lossy().into_owned(),
                    root: project_root.to_string_lossy().into_owned(),
                    writable_paths,
                    worktree: None,
                },
                None,
            )
        }
        AgentWorkspaceMode::Worktree => {
            let backend = backend_for(
                ssh_manager,
                request.project.ssh_server_id.as_deref(),
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
            let path =
                WorktreeManager::allocate_path(&repository_root, request.root_thread_id, &child_id);
            let branch = WorktreeManager::branch_for(&child_id);
            let mut durable = WorktreeLease {
                revision: 1,
                state: WorktreeLeaseState::Prepared,
                child_id: child_id.clone(),
                root_thread_id: request.root_thread_id.to_owned(),
                project_id: request.project.id.clone(),
                ssh_server_id: request.project.ssh_server_id.clone(),
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
                    child_id: child_id.clone(),
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
    ssh_server_id: Option<&str>,
    project_root: &Path,
) -> Arc<dyn WorktreeBackend> {
    match ssh_server_id {
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
        backend_for(
            ssh_manager,
            lease.ssh_server_id.as_deref(),
            &repository_root,
        ),
    )
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
            child_id = lease.child_id,
            error = %persist_error,
            "failed to settle durable worktree lease after create failure"
        );
    }
}

pub(in crate::studio) fn resolved_project_root(project: &ProjectRecord) -> Result<PathBuf> {
    if project.ssh_server_id.is_some() {
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
