use sea_orm::DatabaseConnection;
use std::path::PathBuf;

use crate::studio::paths::StudioPaths;
use crate::studio::storage::coordinator::ThreadPersistenceCoordinator;
use crate::studio::storage::history::HistoryStore;
use crate::studio::storage::{calls::CallsStore, state::StateStore};

mod agent_framework;
pub(crate) mod attachment;
pub(in crate::studio) mod catalog;
pub(in crate::studio) mod directory;
mod error;
mod interaction;
pub(in crate::studio) mod object;
mod project;
pub(in crate::studio) mod settings;
pub(in crate::studio) mod ssh_migration;
mod thread;
pub(in crate::studio) mod workspaces;

#[derive(Clone)]
pub struct StudioStore {
    db: DatabaseConnection,
    calls: CallsStore,
    catalog: crate::studio::catalog::CatalogStore,
    settings: settings::SettingsStore,
    workspaces: workspaces::WorkspaceStore,
    thread_persistence: ThreadPersistenceCoordinator,
    attachment_lock: std::sync::Arc<tokio::sync::Mutex<()>>,
    paths: StudioPaths,
}

pub use error::StudioDatabaseError;
impl StudioStore {
    pub(crate) fn database(&self) -> &DatabaseConnection {
        &self.db
    }

    /// 轻量会话目录摘要事实源；首屏/分页/搜索只读取这里。
    pub(in crate::studio) fn catalog(&self) -> &crate::studio::catalog::CatalogStore {
        &self.catalog
    }

    pub(in crate::studio) fn settings_store(&self) -> &settings::SettingsStore {
        &self.settings
    }

    /// 打开一个 Thread 的 history 句柄。
    ///
    /// 冷读（分页、按身份读条目、Turn 页）只读打开**已存在**的库并校验 schema、Thread 与
    /// 数据库身份，绝不创建目录/数据库或执行 schema 变更；只有显式写入（历史 writer 的
    /// commit/reserve 或迁移 export）才会创建并升级它。句柄本身不做 IO，每次调用返回新句柄，
    /// 用完即随句柄释放，因此不存在无界连接缓存，也不会在启动时 eager 建库。
    pub(crate) async fn history(&self, thread_id: &str) -> anyhow::Result<HistoryStore> {
        HistoryStore::open(
            &self.thread_storage_dir(thread_id).join("history.sqlite"),
            thread_id,
        )
        .await
    }

    /// 该 Thread 的唯一有序历史写者句柄（没有活跃持有者时登记一个新的）。
    ///
    /// effect commit 与实时订阅的 ordinal 预留都必须经由这一个句柄，同一 `history.sqlite` 才不会
    /// 出现两条各自独立的 SQLite writer 互抢写锁。句柄只在真正的持有者（sink、订阅）存活期间存在，
    /// 因此关闭/淘汰 Thread 会释放连接，而不是留下永不失效的强引用缓存；重复打开而未登记的句柄
    /// 只绑定了路径（`HistoryStore::open` 不做 IO），不会产生第二条连接。
    pub(crate) async fn history_writer(&self, thread_id: &str) -> anyhow::Result<HistoryStore> {
        if let Some(shared) = self.thread_persistence.shared_history(thread_id) {
            return Ok(shared);
        }
        let opened = self.history(thread_id).await?;
        Ok(self
            .thread_persistence
            .install_shared_history(thread_id, &opened))
    }

    pub(crate) fn state(&self, thread_id: &str) -> StateStore {
        StateStore::new(self.thread_storage_dir(thread_id), thread_id)
    }

    pub(crate) fn calls(&self) -> &CallsStore {
        &self.calls
    }

    pub(crate) fn thread_persistence(&self) -> &ThreadPersistenceCoordinator {
        &self.thread_persistence
    }

    pub(super) fn attachment_lock(&self) -> &tokio::sync::Mutex<()> {
        &self.attachment_lock
    }

    pub(crate) fn thread_storage_dir(&self, thread_id: &str) -> PathBuf {
        self.paths.thread_storage_dir(thread_id)
    }

    /// 新附件与迁移附件共用的每会话 blob 根（content-addressed）。
    pub(crate) fn thread_blobs_dir(&self, thread_id: &str) -> PathBuf {
        self.paths.thread_blobs_dir(thread_id)
    }

    /// Temporary attachment-draft root derived from the canonical Studio layout.
    pub(crate) fn attachment_drafts_dir(&self) -> PathBuf {
        self.paths.attachment_drafts_dir()
    }

    /// 每会话资源（工具持久化媒体等）根目录；与附件 blob 根同属一个会话目录。
    pub(crate) fn session_resources_dir(&self, thread_id: &str) -> PathBuf {
        self.thread_storage_dir(thread_id).join("thread-resources")
    }
}

// Legacy Task persistence tests were removed with the fixed Task runtime.

#[cfg(test)]
mod tests {
    use super::*;

    /// 同一 Thread 的 effect commit 与实时订阅必须拿到同一个写者句柄；登记句柄本身不做 IO，
    /// 因此冷库不会被创建，也不会被升级。
    #[tokio::test]
    async fn history_writer_is_shared_and_does_not_create_cold_databases() {
        let store = StudioStore::open_memory().await.unwrap();
        let path = store.thread_storage_dir("thread-1").join("history.sqlite");

        let first = store.history_writer("thread-1").await.unwrap();
        // sink 与订阅从两条不同调用链取用，但必须是同一个有序写者。
        let second = store.history_writer("thread-1").await.unwrap();
        assert!(first.is_same_handle(&second));

        // 另一个 Thread 是另一个写者身份，绝不共享连接。
        let other = store.history_writer("thread-2").await.unwrap();
        assert!(!other.is_same_handle(&first));

        // 只登记句柄不触发任何 IO：冷库既没有被创建，也没有被升级。
        assert!(
            !path.exists(),
            "登记写者句柄不得创建 history.sqlite 或它的目录"
        );
    }
}
