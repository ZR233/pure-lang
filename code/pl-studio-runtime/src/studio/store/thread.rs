//! Thread 目录记录及其稳定配置的持久化入口。

use anyhow::Result;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
};

use crate::StudioMode;
use crate::studio::entity as entities;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::mappers::thread_record;
use crate::studio::records::ThreadRecord;
use crate::studio::store::StudioStore;
use crate::studio::store_support::non_empty_title;

impl StudioStore {
    pub async fn create_thread(
        &self,
        project_id: &str,
        title: &str,
        mode: StudioMode,
    ) -> Result<ThreadRecord> {
        use entities::thread;
        let now = unix_seconds();
        let id = new_id("thread");
        let usage_json = serde_json::to_string(&pl_model::TokenUsage::default())?;
        let role = match mode {
            StudioMode::Simple => "executor",
            StudioMode::Task => "planner",
        };
        let model = thread::ActiveModel {
            id: Set(id.clone()),
            project_id: Set(project_id.to_string()),
            title: Set(non_empty_title(title)),
            mode: Set(mode.label().to_string()),
            root_thread_id: Set(id.clone()),
            parent_thread_id: Set(None),
            role: Set(role.to_string()),
            agent_path: Set(id),
            status: Set("idle".to_string()),
            revision: Set(0),
            runtime_revision: Set(None),
            event_sequence: Set(0),
            metadata_json: Set("null".to_string()),
            usage_json: Set(usage_json),
            last_context_tokens: Set(None),
            trace_sequence: Set(0),
            created_at: Set(now),
            updated_at: Set(now),
            archived: Set(0),
        }
        .insert(&self.db)
        .await?;
        Ok(thread_record(model))
    }

    pub(in crate::studio) async fn create_child_thread(
        &self,
        spec: ChildThreadSpec,
    ) -> Result<ThreadRecord> {
        use entities::thread;
        anyhow::ensure!(
            spec.id == spec.agent_path,
            "Thread id and runtime identity must be identical"
        );
        if let Some(existing) = thread::Entity::find_by_id(spec.id.clone())
            .one(&self.db)
            .await?
        {
            let existing = thread_record(existing);
            anyhow::ensure!(
                existing.parent_thread_id.as_deref() == Some(spec.parent_thread_id.as_str()),
                "Thread {} 已属于其他父 Thread",
                spec.id
            );
            return Ok(existing);
        }
        let parent = thread::Entity::find_by_id(spec.parent_thread_id.clone())
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow::anyhow!("父 Thread 不存在: {}", spec.parent_thread_id))?;
        let now = unix_seconds();
        let usage_json = serde_json::to_string(&pl_model::TokenUsage::default())?;
        let model = thread::ActiveModel {
            id: Set(spec.id.clone()),
            project_id: Set(parent.project_id),
            title: Set(non_empty_title(&spec.title)),
            mode: Set(parent.mode),
            root_thread_id: Set(parent.root_thread_id),
            parent_thread_id: Set(Some(spec.parent_thread_id)),
            role: Set(spec.role),
            agent_path: Set(spec.id),
            status: Set("running".to_string()),
            revision: Set(0),
            runtime_revision: Set(None),
            event_sequence: Set(0),
            metadata_json: Set("null".to_string()),
            usage_json: Set(usage_json),
            last_context_tokens: Set(None),
            trace_sequence: Set(0),
            created_at: Set(now),
            updated_at: Set(now),
            archived: Set(0),
        }
        .insert(&self.db)
        .await?;
        Ok(thread_record(model))
    }

    pub async fn list_root_threads(&self, project_id: &str) -> Result<Vec<ThreadRecord>> {
        use entities::thread;
        let threads = thread::Entity::find()
            .filter(thread::Column::ProjectId.eq(project_id))
            .filter(thread::Column::Mode.is_in(["simple", "task"]))
            .filter(thread::Column::Archived.eq(0))
            .filter(thread::Column::ParentThreadId.is_null())
            .order_by_desc(thread::Column::UpdatedAt)
            .order_by_desc(thread::Column::Id)
            .all(&self.db)
            .await?;
        Ok(threads.into_iter().map(thread_record).collect())
    }

    pub async fn list_threads(&self, project_id: &str) -> Result<Vec<ThreadRecord>> {
        use entities::thread;
        let threads = thread::Entity::find()
            .filter(thread::Column::ProjectId.eq(project_id))
            .filter(thread::Column::Mode.is_in(["simple", "task"]))
            .filter(thread::Column::Archived.eq(0))
            .order_by_desc(thread::Column::UpdatedAt)
            .order_by_asc(thread::Column::CreatedAt)
            .order_by_asc(thread::Column::Id)
            .all(&self.db)
            .await?;
        Ok(threads.into_iter().map(thread_record).collect())
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
        Ok(thread::Entity::find_by_id(thread_id.to_string())
            .filter(thread::Column::Mode.is_in(["simple", "task"]))
            .one(&self.db)
            .await?
            .map(thread_record))
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

    pub async fn rename_thread(&self, thread_id: &str, title: &str) -> Result<()> {
        use entities::thread;
        if let Some(existing) = thread::Entity::find_by_id(thread_id.to_string())
            .one(&self.db)
            .await?
        {
            let mut active: thread::ActiveModel = existing.into();
            active.title = Set(non_empty_title(title));
            active.updated_at = Set(unix_seconds());
            active.update(&self.db).await?;
        }
        Ok(())
    }

    pub async fn archive_thread(&self, thread_id: &str) -> Result<Option<ThreadRecord>> {
        use entities::thread;
        let Some(existing) = thread::Entity::find_by_id(thread_id.to_string())
            .one(&self.db)
            .await?
        else {
            return Ok(None);
        };
        let archived = thread_record(existing.clone());
        let targets = if existing.parent_thread_id.is_none() {
            thread::Entity::find()
                .filter(thread::Column::RootThreadId.eq(thread_id))
                .all(&self.db)
                .await?
        } else {
            vec![existing]
        };
        let now = unix_seconds();
        for target in targets {
            let mut active: thread::ActiveModel = target.into();
            active.archived = Set(1);
            active.updated_at = Set(now);
            active.update(&self.db).await?;
        }
        Ok(Some(archived))
    }

    pub(in crate::studio) async fn update_thread_status(
        &self,
        thread_id: &str,
        status: &str,
        _summary: Option<String>,
        _error: Option<String>,
        updated_at: i64,
    ) -> Result<()> {
        use entities::thread;
        let Some(existing) = thread::Entity::find_by_id(thread_id.to_string())
            .one(&self.db)
            .await?
        else {
            return Ok(());
        };
        let mut active: thread::ActiveModel = existing.into();
        active.status = Set(status.to_string());
        active.updated_at = Set(updated_at);
        active.update(&self.db).await?;
        Ok(())
    }

    pub async fn set_thread_mode(&self, thread_id: &str, mode: StudioMode) -> Result<()> {
        use entities::thread;
        if let Some(existing) = thread::Entity::find_by_id(thread_id.to_string())
            .one(&self.db)
            .await?
        {
            let mut active: thread::ActiveModel = existing.into();
            active.mode = Set(mode.label().to_string());
            active.role = Set(match mode {
                StudioMode::Simple => "executor".to_string(),
                StudioMode::Task => "planner".to_string(),
            });
            active.updated_at = Set(unix_seconds());
            active.update(&self.db).await?;
        }
        Ok(())
    }

    pub(in crate::studio) async fn repair_root_thread_roles(&self) -> Result<usize> {
        use entities::thread;
        let roots = thread::Entity::find()
            .filter(thread::Column::ParentThreadId.is_null())
            .filter(thread::Column::Mode.is_in(["simple", "task"]))
            .all(&self.db)
            .await?;
        let mut repaired = 0;
        for root in roots {
            let expected_role = match StudioMode::from_label(&root.mode) {
                StudioMode::Simple => "executor",
                StudioMode::Task => "planner",
            };
            if root.role == expected_role {
                continue;
            }
            let mut active: thread::ActiveModel = root.into();
            active.role = Set(expected_role.to_string());
            active.update(&self.db).await?;
            repaired += 1;
        }
        Ok(repaired)
    }
}

#[derive(Debug, Clone)]
pub(in crate::studio) struct ChildThreadSpec {
    pub id: String,
    pub parent_thread_id: String,
    pub agent_path: String,
    pub role: String,
    pub title: String,
}
