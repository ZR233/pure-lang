//! Thread 目录记录的冷读取入口。
//!
//! 目录 mutation 已统一走 `store::directory::DirectoryDelta` 的 write-behind
//! 通道（design/19 §19.2）；本文件只保留命令路径允许的聚合冷加载与分页查询。

use anyhow::Result;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};

#[cfg(test)]
use crate::StudioMode;
use crate::studio::entity as entities;
use crate::studio::mappers::thread_record;
use crate::studio::records::ThreadRecord;
use crate::studio::store::StudioStore;

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
        mode: StudioMode,
    ) -> Result<ThreadRecord> {
        use crate::studio::ids::{new_id, unix_seconds};
        use crate::studio::store_support::non_empty_title;
        use pl_core::AgentState;
        use sea_orm::{ActiveModelTrait, ActiveValue::Set};
        let now = unix_seconds();
        let id = new_id("thread");
        let model = entities::thread::ActiveModel {
            id: Set(id.clone()),
            project_id: Set(project_id.to_string()),
            title: Set(non_empty_title(title)),
            mode: Set(mode.label().to_string()),
            root_thread_id: Set(id.clone()),
            parent_thread_id: Set(None),
            role: Set(crate::config::StudioRole::Planner.key().to_string()),
            agent_path: Set(id),
            state_json: Set(serde_json::to_string(&AgentState::idle())?),
            revision: Set(0),
            runtime_revision: Set(None),
            event_sequence: Set(0),
            metadata_json: Set("{}".to_string()),
            usage_json: Set(serde_json::to_string(&pl_model::TokenUsage::default())?),
            last_context_tokens: Set(None),
            trace_sequence: Set(0),
            created_at: Set(now),
            updated_at: Set(now),
            archived: Set(0),
            ..Default::default()
        }
        .insert(&self.db)
        .await?;
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

    /// Thread 树 activation 同批装载相邻 root，用于归档后的选择回退。
    pub async fn list_root_threads_for_activation(
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
        self.list_root_threads(&root.project_id).await
    }

    /// 一棵 Thread 树的全部未归档成员（按 root_thread_id 直查，不扫全项目）。
    pub async fn list_threads_for_root(&self, root_thread_id: &str) -> Result<Vec<ThreadRecord>> {
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
        use entities::thread;
        thread::Entity::find_by_id(thread_id.to_string())
            .one(&self.db)
            .await?
            .map(thread_record)
            .transpose()
    }

    pub(in crate::studio) async fn read_thread_runtime_revision(
        &self,
        thread_id: &str,
    ) -> Result<u64> {
        use entities::thread;
        let revision = thread::Entity::find_by_id(thread_id.to_string())
            .one(&self.db)
            .await?
            .and_then(|thread| thread.runtime_revision)
            .unwrap_or_default();
        Ok(u64::try_from(revision)?)
    }
}
