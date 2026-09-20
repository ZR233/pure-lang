//! Thread 目录记录的冷读取入口。
//!
//! 目录 mutation 已统一走 `store::directory::DirectoryDelta` 的 write-behind
//! 通道（design/17 §17.2）；本文件只保留命令路径允许的聚合冷加载与分页查询。

use anyhow::Result;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};

use crate::studio::entity as entities;
use crate::studio::mappers::thread_record;
use crate::studio::records::ThreadRecord;
use crate::studio::store::StudioStore;
#[cfg(test)]
use pl_protocol::ThreadModeId;

impl StudioStore {
    /// Directory status is a product-directory fact, never a per-row journal replay.
    ///
    /// The `threads` row already carries the maintained status, error and update time
    /// (`mappers::thread_record` decodes them from `state_json`), so a cold directory read consumes
    /// only that row. A missing historical database therefore yields the stable directory state
    /// instead of a journal scan; it is still never reported as empty history
    /// (design/17 §17.2, design/18 §18.1).
    pub(in crate::studio) async fn with_session_status(
        &self,
        record: ThreadRecord,
    ) -> Result<ThreadRecord> {
        Ok(record)
    }
    /// 测试 seed 入口：直接同步创建 root Thread 行。
    ///
    /// 生产路径的创建必须经 `DirectoryDelta::register_root_thread` +
    /// `ProductEventBus::commit_directory`（内存先行、异步落库）。
    #[cfg(test)]
    pub(crate) async fn create_thread(
        &self,
        project_id: &str,
        title: &str,
        mode: ThreadModeId,
    ) -> Result<ThreadRecord> {
        use crate::studio::ids::{new_id, unix_seconds};
        use crate::studio::records::DirectoryState;
        use crate::studio::store_support::non_empty_title;
        use sea_orm::{ActiveModelTrait, ActiveValue::Set};
        let now = unix_seconds();
        let id = new_id("thread");
        let model = entities::thread::ActiveModel {
            id: Set(id.clone()),
            project_id: Set(project_id.to_string()),
            title: Set(non_empty_title(title)),
            mode: Set(mode.label().to_string()),
            workspace_mode: Set(pl_protocol::ThreadWorkspaceMode::Local.label().to_string()),
            root_thread_id: Set(id.clone()),
            parent_thread_id: Set(None),
            role: Set(crate::config::StudioRole::Planner.key().to_string()),
            agent_path: Set(id),
            state_json: Set(serde_json::to_string(&DirectoryState::idle())?),
            revision: Set(0),
            runtime_revision: Set(None),
            event_sequence: Set(0),
            metadata_json: Set("{}".to_string()),
            usage_json: Set(serde_json::to_string(
                &pl_protocol::InferenceTokenUsage::default(),
            )?),
            last_context_tokens: Set(None),
            trace_sequence: Set(0),
            created_at: Set(now),
            updated_at: Set(now),
            archived: Set(0),
            ..Default::default()
        }
        .insert(&self.db)
        .await?;
        // A created Thread owns its own session database; create it here so later
        // directory reads never confuse "no history yet" with "history missing".
        self.sessions().open_thread(&model.id).await?;
        thread_record(model)
    }

    pub async fn list_root_threads(&self, project_id: &str) -> Result<Vec<ThreadRecord>> {
        use entities::thread;
        let threads = thread::Entity::find()
            .filter(thread::Column::ProjectId.eq(project_id))
            .filter(thread::Column::Archived.eq(0))
            .filter(thread::Column::ParentThreadId.is_null())
            .order_by_desc(thread::Column::UpdatedAt)
            .order_by_desc(thread::Column::Id)
            .all(&self.db)
            .await?;
        threads.into_iter().map(thread_record).collect()
    }

    /// Archive selection reads only directory facts, not historical journals.
    pub async fn list_root_threads_for_archive(
        &self,
        root_thread_id: &str,
    ) -> Result<Vec<ThreadRecord>> {
        use entities::thread;
        let Some(root) = thread::Entity::find_by_id(root_thread_id.to_string())
            .one(&self.db)
            .await?
        else {
            return Ok(Vec::new());
        };
        let roots = thread::Entity::find()
            .filter(thread::Column::ProjectId.eq(root.project_id))
            .filter(thread::Column::Archived.eq(0))
            .filter(thread::Column::ParentThreadId.is_null())
            .order_by_desc(thread::Column::UpdatedAt)
            .order_by_desc(thread::Column::Id)
            .all(&self.db)
            .await?;
        roots.into_iter().map(thread_record).collect()
    }

    /// Archive scope includes every active descendant without replaying its journal.
    pub async fn list_threads_for_archive(
        &self,
        root_thread_id: &str,
    ) -> Result<Vec<ThreadRecord>> {
        use entities::thread;
        let threads = thread::Entity::find()
            .filter(thread::Column::RootThreadId.eq(root_thread_id))
            .filter(thread::Column::Archived.eq(0))
            .order_by_asc(thread::Column::CreatedAt)
            .order_by_asc(thread::Column::Id)
            .all(&self.db)
            .await?;
        threads.into_iter().map(thread_record).collect()
    }

    /// Runtime observation needs replayed status; archiving uses the directory-only variant.
    ///
    /// Directory rows already carry the maintained status, so this is the same directory-only read.
    pub async fn list_threads_for_root(&self, root_thread_id: &str) -> Result<Vec<ThreadRecord>> {
        self.list_threads_for_archive(root_thread_id).await
    }

    /// Cold baseline for explicit archive restoration, including archived descendants.
    pub(in crate::studio) async fn read_directory_tree(
        &self,
        root_id: &str,
    ) -> Result<Vec<ThreadRecord>> {
        use entities::thread;
        let rows = thread::Entity::find()
            .filter(thread::Column::RootThreadId.eq(root_id))
            .order_by_asc(thread::Column::CreatedAt)
            .all(&self.db)
            .await?;
        rows.into_iter().map(thread_record).collect()
    }

    /// Project 归档 activation 一次性装载其完整 Thread 目录。
    pub async fn list_threads_for_project(&self, project_id: &str) -> Result<Vec<ThreadRecord>> {
        use entities::thread;
        let threads = thread::Entity::find()
            .filter(thread::Column::ProjectId.eq(project_id))
            .order_by_asc(thread::Column::CreatedAt)
            .order_by_asc(thread::Column::Id)
            .all(&self.db)
            .await?;
        threads.into_iter().map(thread_record).collect()
    }

    pub async fn list_project_thread_ids(&self, project_id: &str) -> Result<Vec<String>> {
        use entities::thread;
        Ok(thread::Entity::find()
            .filter(thread::Column::ProjectId.eq(project_id))
            .all(&self.db)
            .await?
            .into_iter()
            .map(|thread| thread.id)
            .collect())
    }

    pub async fn read_thread(&self, thread_id: &str) -> Result<Option<ThreadRecord>> {
        match self.read_thread_association(thread_id).await? {
            Some(record) => Ok(Some(self.with_session_status(record).await?)),
            None => Ok(None),
        }
    }

    pub(in crate::studio) async fn read_thread_association(
        &self,
        thread_id: &str,
    ) -> Result<Option<ThreadRecord>> {
        use entities::thread;
        match thread::Entity::find_by_id(thread_id.to_string())
            .one(&self.db)
            .await?
        {
            Some(row) => Ok(Some(thread_record(row)?)),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_core::{
        context::OpaquePayload,
        model::{DynModelSession, ModelError, ModelRequest, ModelSession, PreparedModelCall},
        thread::{ThreadHandle, cold::ColdStoreHandle, extensions::ExtensionMutation},
    };
    use pretty_assertions::assert_eq;

    struct NoExecution;
    impl ModelSession for NoExecution {
        async fn prepare(&mut self, _: ModelRequest) -> Result<PreparedModelCall, ModelError> {
            panic!("cold directory reads must never activate a model")
        }
        async fn close(&mut self) -> Result<(), ModelError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn unknown_saved_mode_keeps_known_directory_metadata_readable() {
        let store = StudioStore::open_memory().await.unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let project = store.upsert_project(workspace.path()).await.unwrap();
        let record = store
            .create_thread(&project.id, "cold mode", ThreadModeId::simple())
            .await
            .unwrap();
        let thread =
            ThreadHandle::start(record.id.clone(), DynModelSession::new(NoExecution)).unwrap();
        thread
            .attach_storage(ColdStoreHandle::new(
                store.sessions().open_thread(&record.id).await.unwrap(),
            ))
            .await
            .unwrap();
        let original =
            OpaquePayload::new("future.studio.mode", 99, "{\"mode\":\"future\"}").unwrap();
        thread
            .mutate_extensions(vec![ExtensionMutation::Put {
                id: "studio.mode".into(),
                expected_revision: None,
                payload: original.clone(),
            }])
            .await
            .unwrap();
        thread.close().await.unwrap();
        let restored = store.read_thread(&record.id).await.unwrap().unwrap();
        assert_eq!(restored.mode, record.mode);
        assert_eq!(restored.title, record.title);
        // Status is a product-directory fact: a cold directory read never replays the Thread
        // journal to derive it. An unknown saved mode payload is therefore never decoded here and
        // cannot clobber the known directory metadata (design/17 §17.2, design/18 §18.1).
        assert_eq!(restored.status, pl_protocol::ThreadStatus::Idle);
        let replayed = store.sessions().replay_thread(&record.id).await.unwrap();
        assert_eq!(replayed.extensions["studio.mode"].payload, original);
        store.sessions().shutdown().await.unwrap();
    }

    /// A directory read is served from the product directory row alone.
    ///
    /// Removing each Thread's historical database must neither fail the listing nor silently drop
    /// the row: the cold directory scan never opens a journal (design/17 §17.2).
    #[tokio::test]
    async fn directory_listing_never_reads_a_thread_journal() {
        let home = tempfile::tempdir().unwrap();
        let store = StudioStore::open(home.path().join("studio.sqlite"))
            .await
            .unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let project = store.upsert_project(workspace.path()).await.unwrap();
        let first = store
            .create_thread(&project.id, "first", ThreadModeId::simple())
            .await
            .unwrap();
        let second = store
            .create_thread(&project.id, "second", ThreadModeId::simple())
            .await
            .unwrap();
        // Release every per-Thread connection, then delete the historical databases so that any
        // journal read on the listing path would deterministically fail.
        store.sessions().shutdown().await.unwrap();
        for thread in [&first, &second] {
            let path = store
                .sessions()
                .sessions_dir()
                .expect("a file-backed store routes sessions on disk")
                .join(format!("{}.sqlite", thread.id));
            std::fs::remove_file(&path).unwrap();
        }
        let listed = store.list_root_threads(&project.id).await.unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(
            store.read_thread(&first.id).await.unwrap().unwrap().title,
            first.title
        );
    }
}
