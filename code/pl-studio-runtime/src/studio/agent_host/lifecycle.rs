use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use pl_core::{AgentLifecycleAdapter, CloseLifecycleRequest, SpawnLifecycleRequest};

use crate::{CloseDisposition, PureError, Result, WorktreeHandle, WorktreeManager};

use crate::studio::task_coordinator::TaskCoordinator;
use crate::studio::task_coordinator::{StudioTaskSpawnPreparation, StudioTaskSpawnRequest};

use super::resources::{StudioAgentResource, StudioAgentResources};

#[derive(Clone)]
pub(in crate::studio) struct StudioAgentLifecycle {
    coordinator: Arc<TaskCoordinator>,
    resources: StudioAgentResources,
}

pub(in crate::studio) struct StudioSpawnLease {
    agent_id: pl_core::AgentId,
    resource: StudioAgentResource,
}

pub(in crate::studio) struct StudioCloseLease {
    agent_id: pl_core::AgentId,
    resource: Option<StudioAgentResource>,
    commit_started: AtomicBool,
}

impl StudioAgentLifecycle {
    pub(super) fn new(coordinator: Arc<TaskCoordinator>, resources: StudioAgentResources) -> Self {
        Self {
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
        let session_id = request
            .parent
            .active_session_id
            .as_ref()
            .map(ToString::to_string)
            .or_else(|| {
                request
                    .metadata
                    .get("studioSessionId")
                    .and_then(|value| value.as_str())
                    .map(str::to_string)
            })
            .ok_or_else(|| lifecycle_error("spawn has no Studio session boundary"))?;
        let task_name = request
            .metadata
            .get("taskName")
            .or_else(|| request.metadata.get("name"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_else(|| request.child.identity.role.as_str())
            .to_string();
        let owned_paths = request
            .metadata
            .get("ownedPaths")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .map(str::to_string)
            .collect();
        let requested_by_call_id = request
            .metadata
            .get("requestingToolCallId")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("spawn_agent")
            .to_string();
        let studio_request = StudioTaskSpawnRequest {
            agent_id: request.child.identity.id.to_string(),
            session_id: session_id.clone(),
            task_name: task_name.clone(),
            role: request.child.identity.role.to_string(),
            owned_paths,
            requested_by_call_id,
        };
        let preparation = self
            .coordinator
            .prepare_agent_spawn(&studio_request)
            .await?;
        let worktree = create_worktree(&preparation, &studio_request).await?;
        let workspace_root = worktree
            .as_ref()
            .map(|(_, handle)| handle.path.clone())
            .unwrap_or_else(|| {
                request
                    .metadata
                    .get("workspaceRoot")
                    .and_then(serde_json::Value::as_str)
                    .map(std::path::PathBuf::from)
                    .unwrap_or_default()
            });
        Ok(StudioSpawnLease {
            agent_id: request.child.identity.id,
            resource: StudioAgentResource {
                studio_session_id: session_id,
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
            Ok(())
        } else {
            Err(lifecycle_error(failures.join("; ")))
        }
    }

    async fn prepare_close(&self, request: CloseLifecycleRequest) -> Result<Self::CloseLease> {
        Ok(StudioCloseLease {
            agent_id: request.agent.identity.id.clone(),
            resource: self.resources.get(&request.agent.identity.id).await,
            commit_started: AtomicBool::new(false),
        })
    }

    async fn commit_close(&self, lease: &Self::CloseLease) -> Result<()> {
        lease.commit_started.store(true, Ordering::Release);
        let Some(resource) = &lease.resource else {
            return Ok(());
        };
        self.coordinator
            .commit_agent_close(&resource.request, &resource.preparation)
            .await?;
        if let Some((manager, handle)) = &resource.worktree {
            manager
                .close(handle, CloseDisposition::Discard)
                .await
                .map_err(|error| lifecycle_error(error.to_string()))?;
        }
        self.resources.remove(&lease.agent_id).await;
        Ok(())
    }

    async fn rollback_close(&self, lease: Self::CloseLease) -> Result<()> {
        if lease.commit_started.load(Ordering::Acquire) && lease.resource.is_some() {
            return Err(lifecycle_error(
                "Studio agent close cannot restore a committed worktree cleanup",
            ));
        }
        Ok(())
    }
}

async fn create_worktree(
    preparation: &StudioTaskSpawnPreparation,
    request: &StudioTaskSpawnRequest,
) -> Result<Option<(WorktreeManager, WorktreeHandle)>> {
    let Some(spec) = preparation.worktree_spec().cloned() else {
        return Ok(None);
    };
    let manager = WorktreeManager::local(spec.repo_root.clone());
    let handle = manager.create_from_spec(spec).await.map_err(|error| {
        lifecycle_error(format!(
            "failed to create worktree for {}: {error}",
            request.agent_id
        ))
    })?;
    Ok(Some((manager, handle)))
}

fn lifecycle_error(error: impl Into<String>) -> PureError {
    PureError::ToolExecutionFailed {
        tool: "studio_agent_lifecycle".to_string(),
        error: error.into(),
    }
}
