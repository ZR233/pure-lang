use pl_core::{
    AgentLifecycleAdapter, CloseLifecycleRequest, SpawnLifecycleRequest, SpawnRollbackReason,
};

use crate::studio::product_event_bus::ProductEventBus;
use crate::studio::records::ThreadRecord;
use crate::studio::store::directory::RegisteredChildThread;
use crate::studio::{StudioStore, UnregisteredThreadFault};
use crate::{PureError, Result};

use super::resources::{StudioAgentResource, StudioAgentResources};

/// 统一会话框架只为 child Agent 建立普通 Studio Thread，不创建额外的产品编排记录。
#[derive(Clone)]
pub(in crate::studio) struct StudioAgentLifecycle {
    store: StudioStore,
    product_events: ProductEventBus,
    resources: StudioAgentResources,
}

pub(in crate::studio) struct StudioSpawnLease {
    agent_id: pl_core::ThreadId,
    resource: StudioAgentResource,
}

pub(in crate::studio) struct StudioCloseLease {
    agent_id: pl_core::ThreadId,
}

impl StudioAgentLifecycle {
    pub(super) fn new(
        store: StudioStore,
        product_events: ProductEventBus,
        resources: StudioAgentResources,
    ) -> Self {
        Self {
            store,
            product_events,
            resources,
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
        let profile_id = request
            .metadata
            .get("profileId")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| lifecycle_error("spawn metadata has no Agent Profile id"))?;
        if profile_id != request.child.identity.role.as_str() {
            return Err(lifecycle_error(
                "spawn Agent Profile does not match child runtime role",
            ));
        }
        let child_thread_id = request.child_thread_id.to_string();
        self.product_events
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
            .map_err(|error| lifecycle_error(error.to_string()))?;
        Ok(StudioSpawnLease {
            agent_id: request.child.identity.id,
            resource: StudioAgentResource {
                thread_id: child_thread_id,
                assignment_name: profile_id.to_string(),
            },
        })
    }

    async fn activate_spawn(&self, lease: &Self::SpawnLease) -> Result<()> {
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
        match self
            .store
            .fault_unregistered_child_thread(&lease.resource.thread_id, &reason.message)
            .await
            .map_err(|error| lifecycle_error(error.to_string()))?
        {
            UnregisteredThreadFault::Faulted | UnregisteredThreadFault::RuntimeOwned => Ok(()),
        }
    }

    async fn prepare_close(&self, request: CloseLifecycleRequest) -> Result<Self::CloseLease> {
        Ok(StudioCloseLease {
            agent_id: request.agent.identity.id,
        })
    }

    async fn commit_close(&self, lease: &Self::CloseLease) -> Result<()> {
        self.resources.remove(&lease.agent_id).await;
        Ok(())
    }

    async fn rollback_close(&self, _lease: Self::CloseLease) -> Result<()> {
        Ok(())
    }
}

fn lifecycle_error(error: impl Into<String>) -> PureError {
    PureError::ToolExecutionFailed {
        tool: "studio_agent_lifecycle".to_string(),
        error: error.into(),
    }
}
