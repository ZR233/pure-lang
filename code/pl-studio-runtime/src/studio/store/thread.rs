//! Thread 目录记录的冷读取入口。
//!
//! 目录 mutation 已统一走 `store::directory::DirectoryDelta` 的 write-behind
//! 通道（design/17 §17.2）；本文件只保留命令路径允许的聚合冷加载与分页查询。

use anyhow::Result;

use crate::studio::catalog::CatalogEntry;
#[cfg(test)]
use crate::studio::entity as entities;
#[cfg(test)]
use crate::studio::mappers::thread_record;
use crate::studio::records::ThreadRecord;
use crate::studio::store::StudioStore;
#[cfg(test)]
use pl_protocol::ThreadModeId;

impl StudioStore {
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
        let record = thread_record(model)?;
        let thread = pl_protocol::Thread::from(record.clone());
        self.catalog()
            .upsert(CatalogEntry::from_thread(&thread))
            .await?;
        Ok(record)
    }

    pub async fn list_root_threads(&self, project_id: &str) -> Result<Vec<ThreadRecord>> {
        Ok(self
            .catalog()
            .entries()
            .into_iter()
            .filter(|entry| {
                entry.project_id == project_id
                    && !entry.archived
                    && entry.parent_thread_id.is_none()
            })
            .map(|entry| thread_record_from_catalog(&entry))
            .collect())
    }

    /// Archive selection reads only directory facts, not historical journals.
    pub async fn list_root_threads_for_archive(
        &self,
        root_thread_id: &str,
    ) -> Result<Vec<ThreadRecord>> {
        let Some(root) = self.catalog().get(root_thread_id) else {
            return Ok(Vec::new());
        };
        Ok(self
            .catalog()
            .entries()
            .into_iter()
            .filter(|entry| {
                entry.project_id == root.project_id
                    && !entry.archived
                    && entry.parent_thread_id.is_none()
            })
            .map(|entry| thread_record_from_catalog(&entry))
            .collect())
    }

    /// Archive scope includes every active descendant without replaying its journal.
    pub async fn list_threads_for_archive(
        &self,
        root_thread_id: &str,
    ) -> Result<Vec<ThreadRecord>> {
        Ok(catalog_tree(self, root_thread_id, false)
            .into_iter()
            .map(|entry| thread_record_from_catalog(&entry))
            .collect())
    }

    /// Active descendants of one root. The catalog already carries the persisted status summary.
    pub async fn list_threads_for_root(&self, root_thread_id: &str) -> Result<Vec<ThreadRecord>> {
        self.list_threads_for_archive(root_thread_id).await
    }

    /// Cold baseline for explicit archive restoration, including archived descendants.
    pub(in crate::studio) async fn read_directory_tree(
        &self,
        root_id: &str,
    ) -> Result<Vec<ThreadRecord>> {
        Ok(catalog_tree(self, root_id, true)
            .into_iter()
            .map(|entry| thread_record_from_catalog(&entry))
            .collect())
    }

    /// Project 归档 activation 一次性装载其完整 Thread 目录。
    pub async fn list_threads_for_project(&self, project_id: &str) -> Result<Vec<ThreadRecord>> {
        let mut entries = self
            .catalog()
            .entries()
            .into_iter()
            .filter(|entry| entry.project_id == project_id)
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(entries
            .into_iter()
            .map(|entry| thread_record_from_catalog(&entry))
            .collect())
    }

    pub async fn list_project_thread_ids(&self, project_id: &str) -> Result<Vec<String>> {
        Ok(self
            .catalog()
            .entries()
            .into_iter()
            .filter(|entry| entry.project_id == project_id)
            .map(|entry| entry.id)
            .collect())
    }

    pub async fn read_thread(&self, thread_id: &str) -> Result<Option<ThreadRecord>> {
        Ok(self
            .catalog()
            .get(thread_id)
            .map(|entry| thread_record_from_catalog(&entry)))
    }

    pub(in crate::studio) async fn read_thread_association(
        &self,
        thread_id: &str,
    ) -> Result<Option<ThreadRecord>> {
        self.read_thread(thread_id).await
    }
}

fn thread_record_from_catalog(entry: &CatalogEntry) -> ThreadRecord {
    ThreadRecord::from_directory_thread(entry.to_thread())
}

fn catalog_tree(store: &StudioStore, root_id: &str, include_archived: bool) -> Vec<CatalogEntry> {
    let mut entries = store
        .catalog()
        .entries()
        .into_iter()
        .filter(|entry| entry.root_thread_id == root_id && (include_archived || !entry.archived))
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    entries
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
        let persistence = crate::studio::storage::thread_writer::ThreadStorageSink::new(
            store.clone(),
            pl_protocol::Thread::from(record.clone()),
        );
        thread
            .attach_storage(ColdStoreHandle::new(persistence))
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
        // catalog 的 `status` 只是“最后一次已提交的目录摘要”，由产品观察层刷新；本单测不装观察层，
        // 因此它保持创建时的摘要值，不能当作冷执行权威。
        assert_eq!(restored.status, pl_protocol::ThreadStatus::Idle);
        let checkpoint = store.state(&record.id).load().await.unwrap().unwrap();
        // 冷执行的权威是 checkpoint 的 lifecycle 与产品投影。
        assert_eq!(
            checkpoint.state.lifecycle,
            pl_core::thread::ThreadLifecycle::Closed
        );
        assert_eq!(
            crate::studio::thread_projection::status(&checkpoint.state),
            pl_protocol::ThreadStatus::Closed
        );
        assert_eq!(checkpoint.state.extensions["studio.mode"].payload, original);
    }
}
