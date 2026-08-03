use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use pl_core::{AgentLifecycleAdapter, CloseLifecycleRequest, SpawnLifecycleRequest};

use crate::{CloseDisposition, PureError, Result, WorktreeHandle, WorktreeManager};

use crate::studio::task_coordinator::TaskCoordinator;
use crate::studio::task_coordinator::{
    ExecutorCloseDisposition, StudioSpawnIntent, StudioTaskSpawnPreparation, StudioTaskSpawnRequest,
};
use crate::studio::{AgentSessionSpec, StudioStore};

use super::resources::{StudioAgentResource, StudioAgentResources};

#[derive(Clone)]
pub(in crate::studio) struct StudioAgentLifecycle {
    store: StudioStore,
    coordinator: Arc<TaskCoordinator>,
    resources: StudioAgentResources,
}

pub(in crate::studio) struct StudioSpawnLease {
    agent_id: pl_core::AgentId,
    resource: StudioAgentResource,
}

pub(in crate::studio) struct StudioCloseLease {
    resource: Option<StudioAgentResource>,
    disposition: ExecutorCloseDisposition,
    durable_discard_committed: AtomicBool,
}

impl StudioAgentLifecycle {
    pub(super) fn new(
        store: StudioStore,
        coordinator: Arc<TaskCoordinator>,
        resources: StudioAgentResources,
    ) -> Self {
        Self {
            store,
            coordinator,
            resources,
        }
    }
}

impl AgentLifecycleAdapter for StudioAgentLifecycle {
    type Error = PureError;
    type SpawnLease = StudioSpawnLease;
    type CloseLease = StudioCloseLease;

    async fn prepare_spawn(&self, request: SpawnLifecycleRequest) -> Result<Self::SpawnLease> {
        let intent = StudioSpawnIntent::parse(request.metadata.clone())
            .map_err(|error| lifecycle_error(error.to_string()))?;
        let role = request.child.identity.role.as_str();
        intent
            .validate_role(role)
            .map_err(|error| lifecycle_error(error.to_string()))?;
        let parent_session_id = self
            .resources
            .studio_session_id(&request.parent.identity.id)
            .await
            .or_else(|| intent.studio_session_id.clone())
            .ok_or_else(|| lifecycle_error("spawn has no Studio session boundary"))?;
        let parent_session = self
            .store
            .read_session(&parent_session_id)
            .await
            .map_err(|error| lifecycle_error(error.to_string()))?
            .ok_or_else(|| lifecycle_error("spawn parent Studio session does not exist"))?;
        let root_session_id = parent_session.root_session_id.clone();
        if intent.spawn_kind.is_some()
            && intent.studio_session_id.as_deref() != Some(root_session_id.as_str())
        {
            return Err(lifecycle_error(
                "Task spawn intent does not match the parent Studio session",
            ));
        }
        let child_session_id = request.child_session_id.to_string();
        let task_name = intent.task_name(role);
        let studio_request = StudioTaskSpawnRequest {
            agent_id: request.child.identity.id.to_string(),
            session_id: root_session_id.clone(),
            task_name: task_name.clone(),
            role: role.to_string(),
            owned_paths: intent.owned_paths.clone(),
            requested_by_call_id: intent.requesting_tool_call_id(),
        };
        let preparation = self
            .coordinator
            .prepare_agent_spawn(&studio_request)
            .await?;
        let worktree = match create_worktree(&preparation, &studio_request).await {
            Ok(worktree) => worktree,
            Err(error) => {
                return Err(rollback_prepared_spawn(
                    &self.coordinator,
                    &studio_request,
                    &preparation,
                    None,
                    error.to_string(),
                )
                .await);
            }
        };
        let workspace_root = worktree
            .as_ref()
            .map(|(_, handle)| handle.path.clone())
            .unwrap_or_else(|| intent.workspace_root.clone().unwrap_or_default());
        if let Err(error) = self
            .store
            .create_agent_session(AgentSessionSpec {
                id: child_session_id.clone(),
                parent_session_id,
                owner_agent_id: request.child.identity.id.to_string(),
                owner_role: request.child.identity.role.to_string(),
                title: task_name.clone(),
            })
            .await
        {
            return Err(rollback_prepared_spawn(
                &self.coordinator,
                &studio_request,
                &preparation,
                worktree.as_ref(),
                format!("failed to create Studio child session: {error}"),
            )
            .await);
        }
        Ok(StudioSpawnLease {
            agent_id: request.child.identity.id,
            resource: StudioAgentResource {
                studio_session_id: child_session_id,
                workspace_root,
                task_name,
                request: studio_request,
                preparation,
                worktree,
            },
        })
    }

    async fn activate_spawn(&self, lease: &Self::SpawnLease) -> Result<()> {
        self.coordinator
            .activate_agent_spawn(&lease.resource.request, &lease.resource.preparation)
            .await?;
        self.resources
            .insert(lease.agent_id.clone(), lease.resource.clone())
            .await;
        Ok(())
    }

    async fn rollback_spawn(&self, lease: Self::SpawnLease) -> Result<()> {
        self.resources.remove(&lease.agent_id).await;
        let mut failures = Vec::new();
        if let Err(error) = self
            .coordinator
            .rollback_agent_spawn(
                &lease.resource.request,
                &lease.resource.preparation,
                "framework spawn rolled back",
            )
            .await
        {
            failures.push(error.to_string());
        }
        if let Some((manager, handle)) = &lease.resource.worktree
            && let Err(error) = manager.close(handle, CloseDisposition::Discard).await
        {
            failures.push(error.to_string());
        }
        if failures.is_empty() {
            self.store
                .update_agent_session_status(
                    &lease.resource.studio_session_id,
                    "faulted",
                    None,
                    Some("framework spawn rolled back".to_string()),
                    crate::studio::ids::unix_seconds(),
                )
                .await
                .map_err(|error| lifecycle_error(error.to_string()))?;
            Ok(())
        } else {
            Err(lifecycle_error(failures.join("; ")))
        }
    }

    async fn prepare_close(&self, request: CloseLifecycleRequest) -> Result<Self::CloseLease> {
        let resource = self.resources.get(&request.agent.identity.id).await;
        let disposition = match resource.as_ref() {
            Some(resource) => {
                self.coordinator
                    .prepare_agent_close(&resource.request, &resource.preparation)
                    .await?
            }
            None => ExecutorCloseDisposition::Discard,
        };
        Ok(StudioCloseLease {
            resource,
            disposition,
            durable_discard_committed: AtomicBool::new(false),
        })
    }

    async fn commit_close(&self, lease: &Self::CloseLease) -> Result<()> {
        let Some(resource) = &lease.resource else {
            return Ok(());
        };
        let disposition = self
            .coordinator
            .commit_agent_close(&resource.request, &resource.preparation)
            .await?;
        if disposition != lease.disposition {
            return Err(lifecycle_error(
                "Studio agent close disposition changed after preflight",
            ));
        }
        if disposition == ExecutorCloseDisposition::Discard && resource.request.role == "executor" {
            lease
                .durable_discard_committed
                .store(true, Ordering::Release);
        }
        if disposition == ExecutorCloseDisposition::Discard
            && let Some((manager, handle)) = &resource.worktree
        {
            manager
                .close(handle, CloseDisposition::Discard)
                .await
                .map_err(|error| lifecycle_error(error.to_string()))?;
        }
        Ok(())
    }

    async fn rollback_close(&self, lease: Self::CloseLease) -> Result<()> {
        if lease.durable_discard_committed.load(Ordering::Acquire) {
            return Err(lifecycle_error(
                "Studio agent close cannot restore a committed executor discard",
            ));
        }
        Ok(())
    }
}

async fn rollback_prepared_spawn(
    coordinator: &TaskCoordinator,
    request: &StudioTaskSpawnRequest,
    preparation: &StudioTaskSpawnPreparation,
    worktree: Option<&(WorktreeManager, WorktreeHandle)>,
    primary_error: String,
) -> PureError {
    let mut failures = vec![primary_error.clone()];
    if let Err(error) = coordinator
        .rollback_agent_spawn(request, preparation, &primary_error)
        .await
    {
        failures.push(format!("spawn allocation rollback failed: {error}"));
    }
    if let Some((manager, handle)) = worktree
        && let Err(error) = manager.close(handle, CloseDisposition::Discard).await
    {
        failures.push(format!("worktree cleanup failed: {error}"));
    }
    lifecycle_error(failures.join("; "))
}

async fn create_worktree(
    preparation: &StudioTaskSpawnPreparation,
    request: &StudioTaskSpawnRequest,
) -> std::result::Result<Option<(WorktreeManager, WorktreeHandle)>, String> {
    let Some(spec) = preparation.worktree_spec().cloned() else {
        return Ok(None);
    };
    let manager = WorktreeManager::local(spec.repo_root.clone());
    let handle = manager.create_from_spec(spec).await.map_err(|error| {
        format!(
            "failed to create worktree for {}: {error}",
            request.agent_id
        )
    })?;
    Ok(Some((manager, handle)))
}

fn lifecycle_error(error: impl Into<String>) -> PureError {
    PureError::ToolExecutionFailed {
        tool: "studio_agent_lifecycle".to_string(),
        error: error.into(),
    }
}
