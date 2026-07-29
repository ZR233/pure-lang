use std::collections::{BTreeMap, BTreeSet};
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
    session_bindings: Arc<RwLock<BTreeMap<AgentId, String>>>,
    cleanup_takeovers: Arc<RwLock<BTreeSet<String>>>,
}

impl StudioAgentResources {
    pub(super) async fn insert(&self, id: AgentId, resource: StudioAgentResource) {
        self.session_bindings
            .write()
            .await
            .insert(id.clone(), resource.studio_session_id.clone());
        self.entries.write().await.insert(id, resource);
    }

    pub(super) async fn get(&self, id: &AgentId) -> Option<StudioAgentResource> {
        self.entries.read().await.get(id).cloned()
    }

    pub(super) async fn remove(&self, id: &AgentId) -> Option<StudioAgentResource> {
        self.entries.write().await.remove(id)
    }

    pub(in crate::studio) async fn begin_cleanup_takeover(
        &self,
        root_session_ids: &BTreeSet<String>,
    ) {
        self.cleanup_takeovers
            .write()
            .await
            .extend(root_session_ids.iter().cloned());
    }

    pub(super) async fn release_after_close(&self, id: &AgentId) {
        let takeovers = self.cleanup_takeovers.read().await;
        let mut entries = self.entries.write().await;
        let preserve = entries
            .get(id)
            .is_some_and(|resource| takeovers.contains(&resource.request.session_id));
        if !preserve {
            entries.remove(id);
        }
    }

    pub(in crate::studio) async fn complete_cleanup_takeover(
        &self,
        root_session_ids: &BTreeSet<String>,
    ) {
        let mut takeovers = self.cleanup_takeovers.write().await;
        let removed_agent_ids = {
            let mut entries = self.entries.write().await;
            let removed = entries
                .iter()
                .filter(|(_, resource)| root_session_ids.contains(&resource.request.session_id))
                .map(|(agent_id, _)| agent_id.clone())
                .collect::<Vec<_>>();
            for agent_id in &removed {
                entries.remove(agent_id);
            }
            removed
        };
        let mut bindings = self.session_bindings.write().await;
        for agent_id in removed_agent_ids {
            bindings.remove(&agent_id);
        }
        for session_id in root_session_ids {
            takeovers.remove(session_id);
        }
    }

    pub(in crate::studio) async fn restore_bindings(
        &self,
        sessions: impl IntoIterator<Item = crate::studio::SessionRecord>,
    ) {
        let mut bindings = self.session_bindings.write().await;
        for session in sessions {
            let Ok(agent_id) = AgentId::new(session.owner_agent_id) else {
                continue;
            };
            bindings.insert(agent_id, session.id);
        }
    }

    pub(super) async fn studio_session_id(&self, id: &AgentId) -> Option<String> {
        if let Some(session_id) = root_session_id(id) {
            return Some(session_id);
        }
        if let Some(session_id) = self.session_bindings.read().await.get(id).cloned() {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cleanup_takeover_preserves_resource_until_durable_cleanup_completes() {
        let resources = StudioAgentResources::default();
        let agent_id = AgentId::new("executor-cleanup-test").unwrap();
        let root_session_id = "root-cleanup-test".to_string();
        resources
            .insert(
                agent_id.clone(),
                StudioAgentResource {
                    studio_session_id: "child-cleanup-test".to_string(),
                    workspace_root: PathBuf::from("C:\\workspace"),
                    task_name: "cleanup test".to_string(),
                    request: StudioTaskSpawnRequest {
                        agent_id: agent_id.to_string(),
                        session_id: root_session_id.clone(),
                        task_name: "cleanup test".to_string(),
                        role: "explorer".to_string(),
                        owned_paths: Vec::new(),
                        requested_by_call_id: "call-cleanup-test".to_string(),
                    },
                    preparation: StudioTaskSpawnPreparation::test_without_worktree(),
                    worktree: None,
                },
            )
            .await;
        let root_session_ids = BTreeSet::from([root_session_id]);

        resources.begin_cleanup_takeover(&root_session_ids).await;
        resources.release_after_close(&agent_id).await;

        assert!(
            resources.get(&agent_id).await.is_some(),
            "a close or close failure during project cleanup must retain durable ownership"
        );
        assert_eq!(
            resources.studio_session_id(&agent_id).await.as_deref(),
            Some("child-cleanup-test")
        );

        resources.complete_cleanup_takeover(&root_session_ids).await;
        assert!(resources.get(&agent_id).await.is_none());
        assert!(resources.studio_session_id(&agent_id).await.is_none());
    }
}
