use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use pl_core::ThreadId;
use tokio::sync::RwLock;

use crate::studio::task_coordinator::{StudioTaskSpawnPreparation, StudioTaskSpawnRequest};
use crate::{WorktreeHandle, WorktreeManager};

#[derive(Clone)]
pub(super) struct StudioAgentResource {
    pub(super) thread_id: String,
    pub(super) task_name: String,
    pub(super) request: StudioTaskSpawnRequest,
    pub(super) preparation: StudioTaskSpawnPreparation,
    pub(super) worktree: Option<(WorktreeManager, WorktreeHandle)>,
}

#[derive(Clone, Default)]
pub(in crate::studio) struct StudioAgentResources {
    entries: Arc<RwLock<BTreeMap<ThreadId, StudioAgentResource>>>,
    cleanup_takeovers: Arc<RwLock<BTreeSet<String>>>,
}

impl StudioAgentResources {
    pub(super) async fn insert(&self, id: ThreadId, resource: StudioAgentResource) {
        self.entries.write().await.insert(id, resource);
    }

    pub(super) async fn get(&self, id: &ThreadId) -> Option<StudioAgentResource> {
        self.entries.read().await.get(id).cloned()
    }

    pub(super) async fn remove(&self, id: &ThreadId) -> Option<StudioAgentResource> {
        self.entries.write().await.remove(id)
    }

    pub(in crate::studio) async fn begin_cleanup_takeover(
        &self,
        root_thread_ids: &BTreeSet<String>,
    ) {
        self.cleanup_takeovers
            .write()
            .await
            .extend(root_thread_ids.iter().cloned());
    }

    pub(super) async fn release_after_close(&self, id: &ThreadId) {
        let takeovers = self.cleanup_takeovers.read().await;
        let mut entries = self.entries.write().await;
        let preserve = entries
            .get(id)
            .is_some_and(|resource| takeovers.contains(&resource.request.root_thread_id));
        if !preserve {
            entries.remove(id);
        }
    }

    pub(in crate::studio) async fn complete_cleanup_takeover(
        &self,
        root_thread_ids: &BTreeSet<String>,
    ) {
        let mut takeovers = self.cleanup_takeovers.write().await;
        {
            let mut entries = self.entries.write().await;
            let removed = entries
                .iter()
                .filter(|(_, resource)| root_thread_ids.contains(&resource.request.root_thread_id))
                .map(|(agent_id, _)| agent_id.clone())
                .collect::<Vec<_>>();
            for agent_id in &removed {
                entries.remove(agent_id);
            }
        }
        for thread_id in root_thread_ids {
            takeovers.remove(thread_id);
        }
    }

    pub(super) async fn thread_id(&self, id: &ThreadId) -> Option<String> {
        Some(id.to_string())
    }
}

pub(in crate::studio) fn root_agent_id(thread_id: &str) -> ThreadId {
    ThreadId::new(thread_id).expect("Studio Thread id 必须非空")
}
