//! Product workspace allocation independent of Agent execution and model sessions.
use super::worktree_lease::{
    WorktreeLease, WorktreeLeaseOwner, WorktreeLeaseOwnerKind, WorktreeLeaseState,
};
use crate::agent::worktree::{
    LocalWorktreeBackend, RemoteWorktreeBackend, WorktreeBackend, WorktreeCreateSpec,
    WorktreeHandle, WorktreeManager, WorktreeOwnership,
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
    let remote = request.project.ssh_alias.is_some();
    if mode != AgentWorkspaceMode::Directory && request.writable_paths.is_some() {
        return Err(lifecycle_error(
            "only directory Profiles accept writablePaths",
        ));
    }
    let writable_paths = normalize_scope(request.writable_paths)?;
    let project_root_text = workspace_path_text(&project_root, remote)?;
    let session_root_text = workspace_path_text(&session_root, remote)?;

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
            let writable_paths = writable_paths
                .map(|paths| {
                    paths
                        .into_iter()
                        .map(|path| {
                            let path = if path == "." {
                                session_root.clone()
                            } else {
                                session_root.join(path)
                            };
                            workspace_path_text(&path, remote)
                        })
                        .collect::<Result<Vec<_>>>()
                })
                .transpose()?;
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
            let (repository_root, manager) = repository_manager(
                ssh_manager,
                request.project.ssh_alias.as_deref(),
                &project_root,
            )
            .await?;
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
            // 远端 lease 的仓库根与 worktree 路径必须记录为 POSIX：会话工作区根、workspace
            // 打开参数与 git 路径参数都从该记录派生，宿主分隔符不得进入 durable 事实。
            let repository_root_text = workspace_path_text(&repository_root, remote)?;
            let mut durable = WorktreeLease {
                revision: 1,
                state: WorktreeLeaseState::Prepared,
                owner_kind: WorktreeLeaseOwnerKind::Child,
                owner_thread_id: child_id.clone(),
                root_thread_id: request.root_thread_id.to_owned(),
                project_id: request.project.id.clone(),
                ssh_alias: request.project.ssh_alias.clone(),
                repository_root: repository_root_text.clone(),
                path: workspace_path_text(&path, remote)?,
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
            let handle_path_text = workspace_path_text(&handle.path, remote)?;
            durable.path = handle_path_text.clone();
            durable.branch = handle.branch.clone();
            (
                AgentWorkspaceAssignmentSnapshot {
                    mode,
                    project_root: project_root_text,
                    root: handle_path_text.clone(),
                    writable_paths: None,
                    worktree: Some(AgentWorktreeSnapshot {
                        repository_root: repository_root_text,
                        path: handle_path_text,
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
) -> Result<Arc<dyn WorktreeBackend>> {
    match ssh_alias {
        Some(server_id) => {
            RemoteWorktreeBackend::new(ssh_manager.clone(), server_id, project_root.to_path_buf())
                .map(|backend| Arc::new(backend) as Arc<dyn WorktreeBackend>)
                .map_err(|error| lifecycle_error(error.to_string()))
        }
        None => Ok(Arc::new(LocalWorktreeBackend::default())),
    }
}

pub(in crate::studio) fn manager_from_lease(
    ssh_manager: &Arc<pl_tool::remote::SshManager>,
    lease: &WorktreeLease,
) -> Result<WorktreeManager> {
    let repository_root = PathBuf::from(&lease.repository_root);
    Ok(WorktreeManager::new(
        repository_root.clone(),
        backend_for(ssh_manager, lease.ssh_alias.as_deref(), &repository_root)?,
    ))
}

/// Resolves a Project's repository root and returns the backend and manager anchored on
/// that resolved root.
///
/// 本地和 SSH 项目都以创建时解析出的仓库根作为 worktree backend 根：远端项目的
/// workspace handle 根、Git 工作目录与相对路径基准都取自该仓库根，而不是配置的 Project
/// 目录，因此 Project 目录是仓库子目录时同样成立。解析本身在 Project 目录上执行，返回的
/// manager 与 `manager_from_lease` 使用同一约定，创建路径因此与恢复、preview 和清理一致。
async fn repository_manager(
    ssh_manager: &Arc<pl_tool::remote::SshManager>,
    ssh_alias: Option<&str>,
    project_root: &Path,
) -> Result<(PathBuf, WorktreeManager)> {
    let resolver = backend_for(ssh_manager, ssh_alias, project_root)?;
    let repository_root = WorktreeManager::resolve_repository_root(resolver.as_ref(), project_root)
        .await
        .map_err(|error| lifecycle_error(error.to_string()))?;
    let backend = backend_for(ssh_manager, ssh_alias, &repository_root)?;
    Ok((
        repository_root.clone(),
        WorktreeManager::new(repository_root, backend),
    ))
}

/// Creates the physical worktree of a root session and records its `prepared` lease.
///
/// Preflight (repository root and `HEAD` resolution over the Project's own backend) runs
/// before any resource exists; the physical create then runs on the backend rooted at that
/// repository root, so local and SSH Projects share one convention. A failed physical
/// create settles the durable lease through the existing
/// `WorktreeCreateFailureDisposition` and never bypasses a Git lock or a registered
/// worktree identity.
pub(in crate::studio) async fn create_root_session_worktree(
    worktrees: &WorktreeLeaseOwner,
    ssh_manager: &Arc<pl_tool::remote::SshManager>,
    project: &ProjectRecord,
    thread_id: &str,
) -> Result<WorktreeLease> {
    let project_root = resolved_project_root(project)?;
    let (repository_root, manager) =
        repository_manager(ssh_manager, project.ssh_alias.as_deref(), &project_root).await?;
    let base_commit = manager
        .resolve_head(&repository_root)
        .await
        .map_err(|error| lifecycle_error(error.to_string()))?;
    let ownership = WorktreeOwnership::Session {
        thread_id: thread_id.to_owned(),
    };
    let path = WorktreeManager::allocate_path(&repository_root, thread_id, &ownership);
    let branch = WorktreeManager::branch_for(&ownership);
    // 远端项目的 durable lease 记录 POSIX 形式；本地项目保留宿主形态。
    let remote = project.ssh_alias.is_some();
    // 会话冷恢复会在同一 owner 上重建 lease；revision 必须相对已有记录递增，
    // 否则 `record` 的版本准入会拒绝重建（design/17 §17.1）。
    let revision = worktrees
        .get(thread_id)
        .map(|lease| lease.revision.saturating_add(1))
        .unwrap_or(1);
    let durable = WorktreeLease {
        revision,
        state: WorktreeLeaseState::Prepared,
        owner_kind: WorktreeLeaseOwnerKind::Session,
        owner_thread_id: thread_id.to_owned(),
        root_thread_id: thread_id.to_owned(),
        project_id: project.id.clone(),
        ssh_alias: project.ssh_alias.clone(),
        repository_root: workspace_path_text(&repository_root, remote)?,
        path: workspace_path_text(&path, remote)?,
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
    if !workspace_paths_equal(&handle.path, &path, remote)? || handle.branch != durable.branch {
        return Err(settle_identity_mismatch(worktrees, durable));
    }
    Ok(durable)
}

/// 收束一个 durable worktree lease：`validate_identity → preview_existing →
/// cleanupRequested → discard → cleaned`，任一步失败回落 `preserved` 并返回错误。
///
/// 这是 child 关闭路径与归档清理共用的唯一实现；`disposition == Preserve` 时只把 lease
/// 收束为 `preserved` 而不删除物理资源，`Cleanup` 才删除 Pure-owned 工作树与分支。
pub(in crate::studio) async fn close_workspace(
    leases: &WorktreeLeaseOwner,
    manager: &WorktreeManager,
    mut lease: WorktreeLease,
    disposition: pl_tool::collaboration::thread::AgentWorkspaceDisposition,
) -> anyhow::Result<()> {
    use pl_tool::collaboration::thread::AgentWorkspaceDisposition;
    if lease.state == WorktreeLeaseState::Cleaned {
        return Ok(());
    }
    lease.transition(WorktreeLeaseState::Preserved);
    leases.record(lease.clone())?;
    match disposition {
        AgentWorkspaceDisposition::Preserve => return Ok(()),
        AgentWorkspaceDisposition::Cleanup => {}
    }
    lease.validate_identity()?;
    let handle = WorktreeHandle {
        path: PathBuf::from(&lease.path),
        branch: lease.branch.clone(),
        base_commit: lease.base_commit.clone(),
    };
    manager.preview_existing(&handle).await?;
    lease.transition(WorktreeLeaseState::CleanupRequested);
    leases.record(lease.clone())?;
    if let Err(error) = manager.discard(&handle).await {
        lease.transition(WorktreeLeaseState::Preserved);
        leases.record(lease)?;
        return Err(error.into());
    }
    lease.transition(WorktreeLeaseState::Cleaned);
    leases.record(lease)?;
    Ok(())
}

/// 冷激活时在同一确定性路径重建一个 child worktree，并记录 renewed 的 `prepared` lease。
///
/// 只在保存的 receipt 对应物理工作树缺失或 lease 已清理时调用；路径与 base 都来自冻结的
/// receipt，因此重建落在与原地址一致的位置，child 地址保持稳定。
pub(in crate::studio) async fn recreate_child_worktree(
    worktrees: &WorktreeLeaseOwner,
    ssh_manager: &Arc<pl_tool::remote::SshManager>,
    project: &ProjectRecord,
    root_thread_id: &str,
    child_id: &str,
    receipt: &AgentWorktreeSnapshot,
) -> Result<WorktreeLease> {
    let repository_root = PathBuf::from(&receipt.repository_root);
    let manager = WorktreeManager::new(
        repository_root.clone(),
        backend_for(ssh_manager, project.ssh_alias.as_deref(), &repository_root)?,
    );
    let ownership = WorktreeOwnership::Child {
        child_id: child_id.to_owned(),
    };
    let path = WorktreeManager::allocate_path(&repository_root, root_thread_id, &ownership);
    let remote = project.ssh_alias.is_some();
    if !workspace_paths_equal(&path, Path::new(&receipt.path), remote)? {
        return Err(lifecycle_error(
            "saved child worktree receipt does not match its Pure-owned deterministic path",
        ));
    }
    let branch = WorktreeManager::branch_for(&ownership);
    let revision = worktrees
        .get(child_id)
        .map(|lease| lease.revision.saturating_add(1))
        .unwrap_or(1);
    let durable = WorktreeLease {
        revision,
        state: WorktreeLeaseState::Prepared,
        owner_kind: WorktreeLeaseOwnerKind::Child,
        owner_thread_id: child_id.to_owned(),
        root_thread_id: root_thread_id.to_owned(),
        project_id: project.id.clone(),
        ssh_alias: project.ssh_alias.clone(),
        repository_root: workspace_path_text(&repository_root, remote)?,
        path: workspace_path_text(Path::new(&receipt.path), remote)?,
        branch: branch.clone(),
        base_commit: receipt.base_commit.clone(),
    };
    worktrees
        .record(durable.clone())
        .map_err(|error| lifecycle_error(error.to_string()))?;
    let handle = match manager
        .create(WorktreeCreateSpec {
            repo_root: repository_root,
            root_thread_id: root_thread_id.to_owned(),
            ownership,
            base_commit: receipt.base_commit.clone(),
        })
        .await
    {
        Ok(handle) => handle,
        Err(error) => {
            settle_failed_create(worktrees, durable, &error).await;
            return Err(lifecycle_error(error.to_string()));
        }
    };
    if !workspace_paths_equal(&handle.path, &path, remote)? || handle.branch != durable.branch {
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
        let value = pl_tool::remote::normalize_remote_absolute_path(project.path.trim())
            .map_err(|error| lifecycle_error(error.to_string()))?;
        if value == "/" {
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

/// 远端项目的跨端路径文本统一为 POSIX；本地项目保留宿主形态。
///
/// 归一化表达本身收敛在 [`pl_tool::remote::normalize_remote_absolute_path`]，这里只负责按
/// 项目形态选择：远端 lease、会话工作区根与 workspace 打开参数必须以同一 POSIX 结果
/// 记录，本地项目不得被改写为 POSIX。
fn workspace_path_text(path: &Path, remote: bool) -> Result<String> {
    if remote {
        pl_tool::remote::normalize_remote_absolute_path(&path.to_string_lossy())
            .map_err(|error| lifecycle_error(error.to_string()))
    } else {
        Ok(path.to_string_lossy().into_owned())
    }
}

fn workspace_paths_equal(left: &Path, right: &Path, remote: bool) -> Result<bool> {
    if remote {
        Ok(workspace_path_text(left, true)? == workspace_path_text(right, true)?)
    } else {
        Ok(left == right)
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

    #[test]
    fn remote_child_receipt_matches_windows_shaped_computed_path() {
        let computed = Path::new(r"\repo\.anywork\worktrees\root-1\child-1");
        let durable = Path::new("/repo/.anywork/worktrees/root-1/child-1");

        assert!(workspace_paths_equal(computed, durable, true).unwrap());
        assert!(!workspace_paths_equal(computed, durable, false).unwrap());
    }
}
