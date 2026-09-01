use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use pl_core::{
    AgentLifecycleAdapter, CloseLifecycleRequest, SpawnLifecycleRequest, SpawnRollbackReason,
};
use pl_protocol::{
    AgentWorkspaceAssignmentSnapshot, AgentWorkspaceDisposition, AgentWorkspaceMode,
    AgentWorktreeSnapshot,
};

use crate::agent::worktree::{
    LocalWorktreeBackend, RemoteWorktreeBackend, WorktreeBackend, WorktreeCreateSpec,
    WorktreeHandle, WorktreeManager,
};
use crate::studio::product_event_bus::ProductEventBus;
use crate::studio::records::{ProjectRecord, ThreadRecord};
use crate::studio::store::directory::RegisteredChildThread;
use crate::studio::{StudioStore, UnregisteredThreadFault};
use crate::{PureError, Result};

use super::resources::{StudioAgentResource, StudioAgentResources};
use super::worktree_lease::{WorktreeLease, WorktreeLeaseState, load_lease, put_lease};

#[derive(Clone)]
pub(in crate::studio) struct StudioAgentLifecycle {
    store: StudioStore,
    product_events: ProductEventBus,
    resources: StudioAgentResources,
    ssh_manager: Arc<pl_core::remote::SshManager>,
}

pub(in crate::studio) struct StudioSpawnLease {
    agent_id: pl_core::ThreadId,
    resource: StudioAgentResource,
    assignment: AgentWorkspaceAssignmentSnapshot,
    worktree: Option<SpawnWorktreeLease>,
}

struct SpawnWorktreeLease {
    manager: WorktreeManager,
    handle: WorktreeHandle,
    lease: WorktreeLease,
}

pub(in crate::studio) struct StudioCloseLease {
    agent_id: pl_core::ThreadId,
    disposition: AgentWorkspaceDisposition,
    worktree: Option<CloseWorktreeLease>,
}

struct CloseWorktreeLease {
    manager: WorktreeManager,
    handle: WorktreeHandle,
    lease: WorktreeLease,
    previous_state: WorktreeLeaseState,
    cleanup_performed: AtomicBool,
}

impl StudioAgentLifecycle {
    pub(super) fn new(
        store: StudioStore,
        product_events: ProductEventBus,
        resources: StudioAgentResources,
        ssh_manager: Arc<pl_core::remote::SshManager>,
    ) -> Self {
        Self {
            store,
            product_events,
            resources,
            ssh_manager,
        }
    }

    async fn project_for_thread(&self, thread: &ThreadRecord) -> Result<ProjectRecord> {
        match self
            .product_events
            .project_snapshot()
            .await
            .into_iter()
            .find(|project| project.id == thread.project_id)
        {
            Some(project) => Ok(project),
            None => self
                .store
                .read_project(&thread.project_id)
                .await
                .map_err(|error| lifecycle_error(error.to_string()))?
                .ok_or_else(|| lifecycle_error("spawn parent Studio project does not exist")),
        }
    }

    fn backend_for(
        &self,
        ssh_server_id: Option<&str>,
        project_root: &Path,
    ) -> Arc<dyn WorktreeBackend> {
        match ssh_server_id {
            Some(server_id) => Arc::new(RemoteWorktreeBackend::new(
                self.ssh_manager.clone(),
                server_id,
                project_root.to_path_buf(),
            )),
            None => Arc::new(LocalWorktreeBackend::default()),
        }
    }

    fn manager_from_lease(&self, lease: &WorktreeLease) -> WorktreeManager {
        let repository_root = PathBuf::from(&lease.repository_root);
        WorktreeManager::new(
            repository_root.clone(),
            self.backend_for(lease.ssh_server_id.as_deref(), &repository_root),
        )
    }

    async fn settle_failed_create(
        &self,
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
        if let Err(persist_error) = put_lease(&self.store, &lease).await {
            tracing::error!(
                child_id = lease.child_id,
                error = %persist_error,
                "failed to settle durable worktree lease after create failure"
            );
        }
    }
}

impl AgentLifecycleAdapter for StudioAgentLifecycle {
    type Error = PureError;
    type SpawnLease = StudioSpawnLease;
    type CloseLease = StudioCloseLease;

    async fn prepare_spawn(&self, request: SpawnLifecycleRequest) -> Result<Self::SpawnLease> {
        let parent_thread_id = self
            .resources
            .thread_id(&request.parent.identity.id)
            .await
            .ok_or_else(|| lifecycle_error("spawn has no Studio Thread boundary"))?;
        let parent_thread = match self.product_events.thread_snapshot(&parent_thread_id) {
            Some(thread) => ThreadRecord::from_directory_thread(thread),
            None => self
                .store
                .read_thread(&parent_thread_id)
                .await
                .map_err(|error| lifecycle_error(error.to_string()))?
                .ok_or_else(|| lifecycle_error("spawn parent Studio Thread does not exist"))?,
        };
        let project = self.project_for_thread(&parent_thread).await?;
        let profile = request
            .agent_profile
            .as_ref()
            .ok_or_else(|| lifecycle_error("spawn has no frozen Agent Profile snapshot"))?;
        let profile_id = profile.profile_id.as_str();
        if profile_id != request.child.identity.role.as_str() {
            return Err(lifecycle_error(
                "spawn Agent Profile does not match child runtime role",
            ));
        }
        if request
            .metadata
            .get("profileId")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|metadata_id| metadata_id != profile_id)
        {
            return Err(lifecycle_error(
                "spawn metadata Agent Profile does not match frozen snapshot",
            ));
        }
        let mode = profile.workspace_mode;
        if let Some(metadata_mode) = request.metadata.get("workspaceMode") {
            let metadata_mode: AgentWorkspaceMode = serde_json::from_value(metadata_mode.clone())
                .map_err(|error| {
                lifecycle_error(format!("invalid workspace mode receipt: {error}"))
            })?;
            if metadata_mode != mode {
                return Err(lifecycle_error(
                    "spawn workspace mode receipt does not match frozen Agent Profile",
                ));
            }
        }
        let writable_paths: Option<Vec<String>> = serde_json::from_value(
            request
                .metadata
                .get("writablePaths")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        )
        .map_err(|error| lifecycle_error(format!("invalid writablePaths receipt: {error}")))?;
        if mode != AgentWorkspaceMode::Directory && writable_paths.is_some() {
            return Err(lifecycle_error(format!(
                "{} Profile received an invalid writablePaths receipt",
                mode.label()
            )));
        }
        let project_root = resolved_project_root(&project)?;
        let child_id = request.child.identity.id.to_string();

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
                let backend = self.backend_for(project.ssh_server_id.as_deref(), &project_root);
                let repository_root =
                    WorktreeManager::resolve_repository_root(backend.as_ref(), &project_root)
                        .await
                        .map_err(|error| lifecycle_error(error.to_string()))?;
                let manager = WorktreeManager::new(repository_root.clone(), backend);
                let base_commit = manager
                    .resolve_head(&repository_root)
                    .await
                    .map_err(|error| lifecycle_error(error.to_string()))?;
                let path = WorktreeManager::allocate_path(
                    &repository_root,
                    &parent_thread.root_thread_id,
                    &child_id,
                );
                let branch = WorktreeManager::branch_for(&child_id);
                let mut durable = WorktreeLease {
                    revision: 1,
                    state: WorktreeLeaseState::Prepared,
                    child_id: child_id.clone(),
                    root_thread_id: parent_thread.root_thread_id.clone(),
                    project_id: project.id.clone(),
                    ssh_server_id: project.ssh_server_id.clone(),
                    repository_root: repository_root.to_string_lossy().into_owned(),
                    path: path.to_string_lossy().into_owned(),
                    branch: branch.clone(),
                    base_commit: base_commit.clone(),
                };
                put_lease(&self.store, &durable)
                    .await
                    .map_err(|error| lifecycle_error(error.to_string()))?;
                let handle = match manager
                    .create(WorktreeCreateSpec {
                        repo_root: repository_root.clone(),
                        root_thread_id: parent_thread.root_thread_id.clone(),
                        child_id: child_id.clone(),
                        base_commit: base_commit.clone(),
                    })
                    .await
                {
                    Ok(handle) => handle,
                    Err(error) => {
                        self.settle_failed_create(durable, &error).await;
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
                    Some(SpawnWorktreeLease {
                        manager,
                        handle,
                        lease: durable,
                    }),
                )
            }
        };

        let child_thread_id = request.child_thread_id.to_string();
        if let Err(error) = self
            .product_events
            .register_child_thread(RegisteredChildThread {
                id: child_thread_id.clone(),
                parent_thread_id,
                root_thread_id: parent_thread.root_thread_id,
                agent_path: request.child.identity.id.to_string(),
                project_id: parent_thread.project_id,
                mode: parent_thread.mode.into(),
                role: profile_id.to_string(),
                title: profile_id.to_string(),
            })
            .await
        {
            if let Some(worktree) = worktree {
                let mut durable = worktree.lease;
                match worktree.manager.discard(&worktree.handle).await {
                    Ok(()) => durable.transition(WorktreeLeaseState::Cleaned),
                    Err(cleanup) => {
                        durable.transition(WorktreeLeaseState::Preserved);
                        let _ = put_lease(&self.store, &durable).await;
                        return Err(lifecycle_error(format!(
                            "child Thread registration failed: {error}; worktree cleanup failed: {cleanup}"
                        )));
                    }
                }
                let _ = put_lease(&self.store, &durable).await;
            }
            return Err(lifecycle_error(error.to_string()));
        }
        Ok(StudioSpawnLease {
            agent_id: request.child.identity.id,
            resource: StudioAgentResource {
                thread_id: child_thread_id,
                assignment_name: profile_id.to_string(),
            },
            assignment,
            worktree,
        })
    }

    fn workspace_assignment(
        &self,
        lease: &Self::SpawnLease,
    ) -> Result<Option<AgentWorkspaceAssignmentSnapshot>> {
        Ok(Some(lease.assignment.clone()))
    }

    fn initial_context(
        &self,
        lease: &Self::SpawnLease,
    ) -> Result<Vec<pl_core::PinnedContextSection>> {
        let warning = match lease.assignment.mode {
            AgentWorkspaceMode::Unrestricted => {
                "This Profile adds no workspace restriction. All access still follows the session Permission Mode."
            }
            AgentWorkspaceMode::Directory => {
                "writablePaths is a cooperative boundary enforced only by Pure built-in file mutation tools. Shell, Git, and MCP can bypass it; do not use them to modify project files outside the assigned paths."
            }
            AgentWorkspaceMode::Worktree => {
                "Work only in the assigned Git worktree. Do not merge, cherry-pick, modify the main workspace, or remove the worktree/branch."
            }
        };
        let receipt = serde_json::to_string_pretty(&lease.assignment)
            .map_err(|error| lifecycle_error(error.to_string()))?;
        Ok(vec![pl_core::context_section(
            "agent.workspace",
            1,
            "Frozen Agent Workspace",
            format!("{warning}\n\nCanonical workspace receipt:\n{receipt}"),
        )?])
    }

    async fn activate_spawn(&self, lease: &Self::SpawnLease) -> Result<()> {
        if let Some(worktree) = &lease.worktree {
            let mut durable = worktree.lease.clone();
            durable.transition(WorktreeLeaseState::Active);
            put_lease(&self.store, &durable)
                .await
                .map_err(|error| lifecycle_error(error.to_string()))?;
        }
        self.resources
            .insert(lease.agent_id.clone(), lease.resource.clone())
            .await;
        Ok(())
    }

    async fn rollback_spawn(
        &self,
        lease: Self::SpawnLease,
        reason: SpawnRollbackReason,
    ) -> Result<()> {
        self.resources.remove(&lease.agent_id).await;
        let thread_result = self
            .store
            .fault_unregistered_child_thread(&lease.resource.thread_id, &reason.message)
            .await
            .map_err(|error| lifecycle_error(error.to_string()))
            .map(|fault| match fault {
                UnregisteredThreadFault::Faulted | UnregisteredThreadFault::RuntimeOwned => (),
            });
        let worktree_result = if let Some(worktree) = lease.worktree {
            let mut durable = worktree.lease;
            match worktree.manager.discard(&worktree.handle).await {
                Ok(()) => {
                    durable.transition(WorktreeLeaseState::Cleaned);
                    put_lease(&self.store, &durable)
                        .await
                        .map_err(|error| lifecycle_error(error.to_string()))
                }
                Err(error) => {
                    durable.transition(WorktreeLeaseState::Preserved);
                    let persist = put_lease(&self.store, &durable).await;
                    Err(lifecycle_error(match persist {
                        Ok(()) => error.to_string(),
                        Err(persist) => format!("{error}; failed to preserve lease: {persist}"),
                    }))
                }
            }
        } else {
            Ok(())
        };
        match (thread_result, worktree_result) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(thread), Ok(())) => Err(thread),
            (Ok(()), Err(worktree)) => Err(worktree),
            (Err(thread), Err(worktree)) => Err(lifecycle_error(format!(
                "thread rollback failed: {thread}; worktree rollback failed: {worktree}"
            ))),
        }
    }

    async fn prepare_close(&self, request: CloseLifecycleRequest) -> Result<Self::CloseLease> {
        let agent_id = request.agent.identity.id;
        let worktree = match load_lease(&self.store, agent_id.as_str())
            .await
            .map_err(|error| lifecycle_error(error.to_string()))?
        {
            Some(mut lease) if lease.state != WorktreeLeaseState::Cleaned => {
                let previous_state = lease.state;
                let manager = self.manager_from_lease(&lease);
                let handle = WorktreeHandle {
                    path: PathBuf::from(&lease.path),
                    branch: lease.branch.clone(),
                    base_commit: lease.base_commit.clone(),
                };
                if request.workspace_disposition == AgentWorkspaceDisposition::Cleanup {
                    lease.transition(WorktreeLeaseState::CleanupRequested);
                    put_lease(&self.store, &lease)
                        .await
                        .map_err(|error| lifecycle_error(error.to_string()))?;
                }
                Some(CloseWorktreeLease {
                    manager,
                    handle,
                    lease,
                    previous_state,
                    cleanup_performed: AtomicBool::new(false),
                })
            }
            Some(_) | None => None,
        };
        Ok(StudioCloseLease {
            agent_id,
            disposition: request.workspace_disposition,
            worktree,
        })
    }

    async fn commit_close(&self, lease: &Self::CloseLease) -> Result<()> {
        if let Some(worktree) = &lease.worktree {
            let mut durable = worktree.lease.clone();
            match lease.disposition {
                AgentWorkspaceDisposition::Preserve => {
                    durable.transition(WorktreeLeaseState::Preserved);
                }
                AgentWorkspaceDisposition::Cleanup => {
                    worktree
                        .manager
                        .discard(&worktree.handle)
                        .await
                        .map_err(|error| lifecycle_error(error.to_string()))?;
                    worktree.cleanup_performed.store(true, Ordering::Release);
                    durable.transition(WorktreeLeaseState::Cleaned);
                }
            }
            put_lease(&self.store, &durable)
                .await
                .map_err(|error| lifecycle_error(error.to_string()))?;
        }
        self.resources.remove(&lease.agent_id).await;
        Ok(())
    }

    async fn rollback_close(&self, lease: Self::CloseLease) -> Result<()> {
        if let Some(worktree) = lease.worktree {
            let mut durable = worktree.lease;
            if worktree.cleanup_performed.load(Ordering::Acquire) {
                durable.transition(WorktreeLeaseState::Cleaned);
                put_lease(&self.store, &durable)
                    .await
                    .map_err(|error| lifecycle_error(error.to_string()))?;
            } else if durable.state == WorktreeLeaseState::CleanupRequested {
                durable.transition(worktree.previous_state);
                put_lease(&self.store, &durable)
                    .await
                    .map_err(|error| lifecycle_error(error.to_string()))?;
            }
        }
        Ok(())
    }
}

fn resolved_project_root(project: &ProjectRecord) -> Result<PathBuf> {
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
        pl_core::resolve_workspace_root(&PathBuf::from(&project.path))
            .map_err(|error| lifecycle_error(error.to_string()))
    }
}

fn lifecycle_error(error: impl Into<String>) -> PureError {
    PureError::ToolExecutionFailed {
        tool: "studio_agent_lifecycle".to_string(),
        error: error.into(),
    }
}
