use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::studio::StudioStore;
use crate::studio::store::object::{PersistedStudioObject, load_objects};
#[cfg(test)]
use crate::studio::store::object::{load_object, put_object};

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

/// worktree lease 的归属类型：根会话自身工作区，或子智能体工作区。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::studio) enum WorktreeLeaseOwnerKind {
    Session,
    Child,
}

impl WorktreeLeaseOwnerKind {
    pub(in crate::studio) const fn label(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::Child => "child",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::studio) struct WorktreeLease {
    pub revision: u64,
    pub state: WorktreeLeaseState,
    pub owner_kind: WorktreeLeaseOwnerKind,
    /// 归属 Thread id：`session` 为根会话自身，`child` 为子智能体 Thread。
    pub owner_thread_id: String,
    pub root_thread_id: String,
    pub project_id: String,
    pub ssh_alias: Option<String>,
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

    /// 该 lease 的 Pure-owned 归属；路径 leaf、分支与身份校验都由它派生。
    pub(in crate::studio) fn ownership(&self) -> crate::agent::worktree::WorktreeOwnership {
        use crate::agent::worktree::WorktreeOwnership;
        match self.owner_kind {
            WorktreeLeaseOwnerKind::Session => WorktreeOwnership::Session {
                thread_id: self.owner_thread_id.clone(),
            },
            WorktreeLeaseOwnerKind::Child => WorktreeOwnership::Child {
                child_id: self.owner_thread_id.clone(),
            },
        }
    }

    /// 按归属校验期望 leaf 与期望 branch，仍拒绝任何非 Pure 分支。
    pub(in crate::studio) fn validate_identity(&self) -> Result<()> {
        use crate::agent::worktree::{WorktreeManager, remote_path_text};
        let repository_root = std::path::Path::new(&self.repository_root);
        let ownership = self.ownership();
        // 两侧统一按跨端 POSIX 形式比较：远端远端 lease 记录 POSIX，而 `allocate_path` 在
        // Windows 宿主会注入 `\`；本地项目两侧都保留宿主形态，归一化后仍相等。
        let expected = remote_path_text(&WorktreeManager::allocate_path(
            repository_root,
            &self.root_thread_id,
            &ownership,
        ));
        anyhow::ensure!(
            remote_path_text(std::path::Path::new(&self.path)) == expected,
            "worktree cleanup refused a mismatched Pure-owned leaf"
        );
        anyhow::ensure!(
            self.branch == WorktreeManager::branch_for(&ownership),
            "worktree cleanup refused a mismatched Pure-owned branch"
        );
        Ok(())
    }
}

impl PersistedStudioObject for WorktreeLease {
    type PersistenceDto = Self;

    const OWNER_KIND: &'static str = "agent";
    const OBJECT_KIND: &'static str = "worktreeLease";
    const SCHEMA_VERSION: i64 = 2;

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

/// Version 1 worktree lease payload.
///
/// Only the Studio schema 20 → 21 migration decodes it; it is never a runtime
/// compatibility entry point.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorktreeLeaseV1 {
    revision: u64,
    state: WorktreeLeaseState,
    child_id: String,
    root_thread_id: String,
    project_id: String,
    ssh_server_id: Option<String>,
    repository_root: String,
    path: String,
    branch: String,
    base_commit: String,
}

/// Rewrites one persisted lease payload from version 1 (`childId`) to version 2
/// (`ownerKind` + `ownerThreadId`).
pub(in crate::studio) fn migrate_lease_payload_v1_to_v2(payload_json: &str) -> Result<String> {
    let legacy: WorktreeLeaseV1 = serde_json::from_str(payload_json)?;
    let migrated = serde_json::json!({
        "revision": legacy.revision,
        "state": legacy.state,
        "ownerKind": WorktreeLeaseOwnerKind::Child,
        "ownerThreadId": legacy.child_id,
        "rootThreadId": legacy.root_thread_id,
        "projectId": legacy.project_id,
        "sshAlias": legacy.ssh_server_id,
        "repositoryRoot": legacy.repository_root,
        "path": legacy.path,
        "branch": legacy.branch,
        "baseCommit": legacy.base_commit,
    });
    Ok(serde_json::to_string(&migrated)?)
}

#[cfg(test)]
pub(in crate::studio) async fn load_lease(
    store: &StudioStore,
    owner_thread_id: &str,
) -> Result<Option<WorktreeLease>> {
    load_object(store.database(), owner_thread_id).await
}

#[cfg(test)]
pub(in crate::studio) async fn put_lease(store: &StudioStore, lease: &WorktreeLease) -> Result<()> {
    put_object(
        store.database(),
        &lease.owner_thread_id,
        lease,
        crate::studio::unix_seconds(),
    )
    .await
}

pub(in crate::studio) async fn load_leases(store: &StudioStore) -> Result<Vec<WorktreeLease>> {
    load_objects(store.database()).await
}

/// Canonical process-local lease owner. Storage never participates in admission.
#[derive(Clone)]
pub(in crate::studio) struct WorktreeLeaseOwner {
    entries: std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<String, WorktreeLease>>>,
    /// 进程内「创建中」标记：不持久化，重启后消失。
    ///
    /// 创建路径在记录 `prepared` 之前设置、在创建收束（成功 `active` 或失败 settle 并发布）
    /// 之后清除；标记存续期间该 lease 既不进入 Recovery，也不可被显式清理，即使 owner
    /// Thread 记录尚未发布。
    creating: std::sync::Arc<std::sync::Mutex<std::collections::BTreeSet<String>>>,
    writer: super::ThreadWriteBehindWriter,
}

impl WorktreeLeaseOwner {
    pub(in crate::studio) fn new(writer: super::ThreadWriteBehindWriter) -> Self {
        Self {
            entries: Default::default(),
            creating: Default::default(),
            writer,
        }
    }

    /// Marks a lease as still being created in this process.
    pub(in crate::studio) fn mark_creating(&self, owner_thread_id: &str) {
        self.creating
            .lock()
            .expect("worktree creating set lock poisoned")
            .insert(owner_thread_id.to_string());
    }

    /// Clears the creating mark once creation converged (active, settled or published).
    pub(in crate::studio) fn clear_creating(&self, owner_thread_id: &str) {
        self.creating
            .lock()
            .expect("worktree creating set lock poisoned")
            .remove(owner_thread_id);
    }

    pub(in crate::studio) fn is_creating(&self, owner_thread_id: &str) -> bool {
        self.creating
            .lock()
            .expect("worktree creating set lock poisoned")
            .contains(owner_thread_id)
    }

    /// 开始一个创建窗口，返回在**所有**出口（含提前返回、取消与 panic 展开）清除标记的守卫。
    pub(in crate::studio) fn creation_guard(&self, owner_thread_id: &str) -> WorktreeCreationGuard {
        WorktreeCreationGuard::new(self, owner_thread_id)
    }

    /// Startup hydration never replaces facts already admitted in this process.
    pub(in crate::studio) fn restore(&self, leases: Vec<WorktreeLease>) {
        let mut entries = self.entries.lock().expect("worktree lease lock poisoned");
        for mut lease in leases {
            if let std::collections::btree_map::Entry::Vacant(entry) =
                entries.entry(lease.owner_thread_id.clone())
            {
                // An interrupted process no longer owns an in-flight cleanup operation.
                // Preserve uncertain physical state for explicit reconciliation.
                if lease.state == WorktreeLeaseState::CleanupRequested {
                    lease.transition(WorktreeLeaseState::Preserved);
                    self.writer.record_worktree_lease(lease.clone());
                }
                entry.insert(lease);
            }
        }
    }

    pub(in crate::studio) fn get(&self, owner_thread_id: &str) -> Option<WorktreeLease> {
        self.entries
            .lock()
            .expect("worktree lease lock poisoned")
            .get(owner_thread_id)
            .cloned()
    }

    pub(in crate::studio) fn snapshot(&self) -> Vec<WorktreeLease> {
        self.entries
            .lock()
            .expect("worktree lease lock poisoned")
            .values()
            .cloned()
            .collect()
    }

    /// Revision admission and queue order share one short, IO-free critical section.
    pub(in crate::studio) fn record(&self, lease: WorktreeLease) -> Result<()> {
        let mut entries = self.entries.lock().expect("worktree lease lock poisoned");
        if let Some(previous) = entries.get(&lease.owner_thread_id) {
            if previous == &lease {
                return Ok(());
            }
            anyhow::ensure!(
                lease.revision
                    == previous
                        .revision
                        .checked_add(1)
                        .ok_or_else(|| anyhow::anyhow!("worktree lease revision exhausted"))?,
                "worktree lease revision conflict for {}",
                lease.owner_thread_id
            );
        } else {
            anyhow::ensure!(
                lease.state == WorktreeLeaseState::Prepared,
                "new worktree lease must be prepared"
            );
        }
        entries.insert(lease.owner_thread_id.clone(), lease.clone());
        self.writer.record_worktree_lease(lease);
        Ok(())
    }
}

/// 进程内创建窗口守卫。
///
/// 创建路径在记录 `prepared` 之前构造它；无论成功、失败、取消还是展开，离开作用域时标记
/// 都会被清除，因此不存在「收束了但标记仍存续」的泄漏。
pub(in crate::studio) struct WorktreeCreationGuard {
    worktrees: WorktreeLeaseOwner,
    owner_thread_id: String,
}

impl WorktreeCreationGuard {
    pub(in crate::studio) fn new(worktrees: &WorktreeLeaseOwner, owner_thread_id: &str) -> Self {
        worktrees.mark_creating(owner_thread_id);
        Self {
            worktrees: worktrees.clone(),
            owner_thread_id: owner_thread_id.to_string(),
        }
    }
}

impl Drop for WorktreeCreationGuard {
    fn drop(&mut self) {
        self.worktrees.clear_creating(&self.owner_thread_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lease() -> WorktreeLease {
        WorktreeLease {
            revision: 1,
            state: WorktreeLeaseState::Prepared,
            owner_kind: WorktreeLeaseOwnerKind::Child,
            owner_thread_id: "child-1".to_string(),
            root_thread_id: "root-1".to_string(),
            project_id: "project-1".to_string(),
            ssh_alias: None,
            repository_root: "/repo".to_string(),
            path: "/repo/.anywork/worktrees/root-1/child-1".to_string(),
            branch: "pure-agent-child-1".to_string(),
            base_commit: "base".to_string(),
        }
    }

    #[test]
    fn version_one_lease_payload_migrates_child_ownership_to_version_two() {
        let legacy = serde_json::json!({
            "revision": 3,
            "state": "preserved",
            "childId": "child-1",
            "rootThreadId": "root-1",
            "projectId": "project-1",
            "sshServerId": null,
            "repositoryRoot": "/repo",
            "path": "/repo/.anywork/worktrees/root-1/child-1",
            "branch": "pure-agent-child-1",
            "baseCommit": "base",
        });
        let migrated = migrate_lease_payload_v1_to_v2(&legacy.to_string()).unwrap();
        let decoded: WorktreeLease = serde_json::from_str(&migrated).unwrap();
        assert_eq!(decoded.owner_kind, WorktreeLeaseOwnerKind::Child);
        assert_eq!(decoded.owner_thread_id, "child-1");
        assert_eq!(decoded.state, WorktreeLeaseState::Preserved);
        assert_eq!(decoded.revision, 3);
        assert!(decoded.validate_identity().is_ok());
        assert!(!migrated.contains("childId"));
    }

    #[test]
    fn session_lease_identity_follows_its_ownership() {
        let mut value = lease();
        value.owner_kind = WorktreeLeaseOwnerKind::Session;
        value.owner_thread_id = "root-1".to_string();
        value.path = "/repo/.anywork/worktrees/root-1/session".to_string();
        value.branch = "pure-session-root-1".to_string();
        assert!(value.validate_identity().is_ok());
        value.branch = "pure-agent-root-1".to_string();
        assert!(value.validate_identity().is_err());
    }

    /// 创建窗口守卫必须在任何出口清除标记，且键按 owner id 隔离。
    #[tokio::test]
    async fn creation_guard_clears_the_mark_on_every_exit() {
        let store = StudioStore::open_memory().await.unwrap();
        let worktrees = WorktreeLeaseOwner::new(super::super::ThreadWriteBehindWriter::new(store));
        {
            let _guard = worktrees.creation_guard("child-1");
            assert!(worktrees.is_creating("child-1"));
            assert!(
                !worktrees.is_creating("root-1"),
                "creation marks are isolated per owner id"
            );
        }
        assert!(!worktrees.is_creating("child-1"));
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
