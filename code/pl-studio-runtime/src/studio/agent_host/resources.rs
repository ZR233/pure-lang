use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use pl_core::AgentId;
use tokio::sync::RwLock;

use crate::studio::task_coordinator::{StudioTaskSpawnPreparation, StudioTaskSpawnRequest};
use crate::{WorktreeHandle, WorktreeManager};

#[derive(Clone)]
pub(super) struct StudioAgentResource {
    pub(super) studio_session_id: String,
    pub(super) workspace_root: PathBuf,
    pub(super) task_name: String,
    pub(super) request: StudioTaskSpawnRequest,
    pub(super) preparation: StudioTaskSpawnPreparation,
    pub(super) worktree: Option<(WorktreeManager, WorktreeHandle)>,
}

#[derive(Clone, Default)]
pub(in crate::studio) struct StudioAgentResources {
    entries: Arc<RwLock<BTreeMap<AgentId, StudioAgentResource>>>,
}

impl StudioAgentResources {
    pub(super) async fn insert(&self, id: AgentId, resource: StudioAgentResource) {
        self.entries.write().await.insert(id, resource);
    }

    pub(super) async fn get(&self, id: &AgentId) -> Option<StudioAgentResource> {
        self.entries.read().await.get(id).cloned()
    }

    pub(super) async fn remove(&self, id: &AgentId) -> Option<StudioAgentResource> {
        self.entries.write().await.remove(id)
    }

    pub(super) async fn studio_session_id(&self, id: &AgentId) -> Option<String> {
        if let Some(session_id) = root_session_id(id) {
            return Some(session_id);
        }
        self.get(id)
            .await
            .map(|resource| resource.studio_session_id)
    }

    pub(super) async fn workspace_root(&self, id: &AgentId) -> Option<PathBuf> {
        self.get(id).await.map(|resource| resource.workspace_root)
    }
}

pub(in crate::studio) fn root_agent_id(session_id: &str) -> AgentId {
    AgentId::new(format!("studio:{session_id}")).expect("Studio session id 必须非空")
}

pub(super) fn root_session_id(agent_id: &AgentId) -> Option<String> {
    agent_id
        .as_str()
        .strip_prefix("studio:")
        .map(str::to_string)
}
