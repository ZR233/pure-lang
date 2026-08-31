use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::studio::StudioStore;
use crate::studio::store::object::{PersistedStudioObject, load_object, load_objects, put_object};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::studio) enum WorktreeLeaseState {
    Prepared,
    Active,
    Preserved,
    CleanupRequested,
    Cleaned,
}

impl WorktreeLeaseState {
    pub(in crate::studio) const fn label(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::Active => "active",
            Self::Preserved => "preserved",
            Self::CleanupRequested => "cleanupRequested",
            Self::Cleaned => "cleaned",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::studio) struct WorktreeLease {
    pub revision: u64,
    pub state: WorktreeLeaseState,
    pub child_id: String,
    pub root_thread_id: String,
    pub project_id: String,
    pub ssh_server_id: Option<String>,
    pub repository_root: String,
    pub path: String,
    pub branch: String,
    pub base_commit: String,
}

impl WorktreeLease {
    pub fn transition(&mut self, state: WorktreeLeaseState) {
        self.revision = self.revision.saturating_add(1);
        self.state = state;
    }
}

impl PersistedStudioObject for WorktreeLease {
    type PersistenceDto = Self;

    const OWNER_KIND: &'static str = "agent";
    const OBJECT_KIND: &'static str = "worktreeLease";
    const SCHEMA_VERSION: i64 = 1;

    fn revision(&self) -> u64 {
        self.revision
    }

    fn to_persistence_dto(&self) -> Self::PersistenceDto {
        self.clone()
    }

    fn from_persistence_dto(dto: Self::PersistenceDto) -> Result<Self> {
        Ok(dto)
    }
}

pub(in crate::studio) async fn load_lease(
    store: &StudioStore,
    child_id: &str,
) -> Result<Option<WorktreeLease>> {
    load_object(store.database(), child_id).await
}

pub(in crate::studio) async fn put_lease(store: &StudioStore, lease: &WorktreeLease) -> Result<()> {
    put_object(
        store.database(),
        &lease.child_id,
        lease,
        crate::studio::unix_seconds(),
    )
    .await
}

pub(in crate::studio) async fn load_leases(store: &StudioStore) -> Result<Vec<WorktreeLease>> {
    load_objects(store.database()).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lease() -> WorktreeLease {
        WorktreeLease {
            revision: 1,
            state: WorktreeLeaseState::Prepared,
            child_id: "child-1".to_string(),
            root_thread_id: "root-1".to_string(),
            project_id: "project-1".to_string(),
            ssh_server_id: None,
            repository_root: "/repo".to_string(),
            path: "/repo/.pure/worktrees/root-1/child-1".to_string(),
            branch: "pure-agent-child-1".to_string(),
            base_commit: "base".to_string(),
        }
    }

    #[tokio::test]
    async fn durable_worktree_lease_preserves_state_and_revision_for_restart_reconcile() {
        let store = StudioStore::open_memory().await.unwrap();
        let mut value = lease();
        put_lease(&store, &value).await.unwrap();
        value.transition(WorktreeLeaseState::Active);
        put_lease(&store, &value).await.unwrap();
        value.transition(WorktreeLeaseState::Preserved);
        put_lease(&store, &value).await.unwrap();

        assert_eq!(
            load_lease(&store, "child-1").await.unwrap(),
            Some(value.clone())
        );
        assert_eq!(load_leases(&store).await.unwrap(), vec![value]);
    }
}
