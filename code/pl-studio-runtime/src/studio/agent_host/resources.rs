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
    tool_sets: Arc<RwLock<BTreeMap<ThreadId, pl_core::AgentToolSet>>>,
    cleanup_takeovers: Arc<RwLock<BTreeSet<String>>>,
    initial_remote_urls: Arc<RwLock<BTreeMap<String, String>>>,
}

impl StudioAgentResources {
    pub(super) async fn insert(&self, id: ThreadId, resource: StudioAgentResource) {
        self.entries.write().await.insert(id, resource);
    }

    pub(super) async fn get(&self, id: &ThreadId) -> Option<StudioAgentResource> {
        self.entries.read().await.get(id).cloned()
    }

    pub(super) async fn remove(&self, id: &ThreadId) -> Option<StudioAgentResource> {
        self.tool_sets.write().await.remove(id);
        self.entries.write().await.remove(id)
    }

    pub(super) async fn tool_set(
        &self,
        id: &ThreadId,
        manager: &pl_core::ToolManager,
    ) -> pl_core::AgentToolSet {
        let mut sets = self.tool_sets.write().await;
        sets.entry(id.clone())
            .or_insert_with(|| {
                manager.agent_tool_set(id.to_string(), pl_core::GlobalToolInheritance::Isolated)
            })
            .clone()
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
            drop(entries);
            self.tool_sets.write().await.remove(id);
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
            drop(entries);
            let mut tool_sets = self.tool_sets.write().await;
            for agent_id in &removed {
                tool_sets.remove(agent_id);
            }
        }
        for thread_id in root_thread_ids {
            takeovers.remove(thread_id);
        }
    }

    pub(super) async fn thread_id(&self, id: &ThreadId) -> Option<String> {
        Some(id.to_string())
    }

    pub(in crate::studio) async fn insert_initial_remote_urls(
        &self,
        urls: impl IntoIterator<Item = (String, String)>,
    ) {
        self.initial_remote_urls.write().await.extend(urls);
    }

    pub(super) async fn take_initial_remote_urls(
        &self,
        attachment_ids: &[String],
    ) -> BTreeMap<String, String> {
        let mut urls = self.initial_remote_urls.write().await;
        attachment_ids
            .iter()
            .filter_map(|attachment_id| {
                urls.remove(attachment_id)
                    .map(|url| (attachment_id.clone(), url))
            })
            .collect()
    }

    pub(in crate::studio) async fn remove_initial_remote_urls(&self, attachment_ids: &[String]) {
        let mut urls = self.initial_remote_urls.write().await;
        for attachment_id in attachment_ids {
            urls.remove(attachment_id);
        }
    }
}

pub(in crate::studio) fn root_agent_id(thread_id: &str) -> ThreadId {
    ThreadId::new(thread_id).expect("Studio Thread id 必须非空")
}
