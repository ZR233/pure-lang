use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectOptions, Database, EntityTrait,
    QueryFilter, QueryOrder, TransactionTrait,
};

use crate::studio::entities;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::mappers::project_record;
use crate::studio::paths::{default_db_path, project_name, sqlite_url};
use crate::studio::records::ProjectRecord;
use crate::studio::store::StudioStore;
use crate::studio::store_support::{configure_sqlite, run_migrations};

impl StudioStore {
    pub async fn default_app() -> Result<Self> {
        let db_path = default_db_path()?;
        Self::open(db_path).await
    }

    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let url = sqlite_url(path);
        Self::open_url(&url).await
    }

    pub async fn open_memory() -> Result<Self> {
        Self::open_url("sqlite::memory:").await
    }

    async fn open_url(url: &str) -> Result<Self> {
        let mut options = ConnectOptions::new(url.to_string());
        options
            .max_connections(1)
            .min_connections(1)
            .connect_timeout(Duration::from_secs(8))
            .acquire_timeout(Duration::from_secs(8))
            .sqlx_logging(false);
        let db = Database::connect(options).await?;
        configure_sqlite(&db).await?;
        run_migrations(&db).await?;
        Ok(Self { db })
    }

    pub async fn upsert_project(&self, path: impl AsRef<Path>) -> Result<ProjectRecord> {
        use entities::project;
        let now = unix_seconds();
        let path = path.as_ref();
        let path_text = path.to_string_lossy().to_string();
        let name = project_name(path);
        if let Some(existing) = project::Entity::find()
            .filter(project::Column::Path.eq(path_text.clone()))
            .one(&self.db)
            .await?
        {
            let mut active: project::ActiveModel = existing.into();
            active.name = Set(name);
            active.updated_at = Set(now);
            active.last_opened_at = Set(Some(now));
            active.closed = Set(0);
            let model = active.update(&self.db).await?;
            return Ok(project_record(model));
        }

        let model = project::ActiveModel {
            id: Set(new_id("project")),
            name: Set(name),
            path: Set(path_text),
            created_at: Set(now),
            updated_at: Set(now),
            last_opened_at: Set(Some(now)),
            closed: Set(0),
        }
        .insert(&self.db)
        .await?;
        Ok(project_record(model))
    }

    pub async fn list_projects(&self) -> Result<Vec<ProjectRecord>> {
        use entities::project;
        let projects = project::Entity::find()
            .filter(project::Column::Closed.eq(0))
            .order_by_desc(project::Column::LastOpenedAt)
            .order_by_desc(project::Column::UpdatedAt)
            .order_by_desc(project::Column::Id)
            .all(&self.db)
            .await?;
        Ok(projects.into_iter().map(project_record).collect())
    }

    pub async fn has_projects(&self) -> Result<bool> {
        use entities::project;
        Ok(project::Entity::find().one(&self.db).await?.is_some())
    }

    pub async fn mark_project_opened(&self, project_id: &str) -> Result<()> {
        use entities::project;
        if let Some(project) = project::Entity::find_by_id(project_id.to_string())
            .one(&self.db)
            .await?
        {
            let now = unix_seconds();
            let mut active: project::ActiveModel = project.into();
            active.updated_at = Set(now);
            active.last_opened_at = Set(Some(now));
            active.closed = Set(0);
            active.update(&self.db).await?;
        }
        Ok(())
    }

    pub async fn read_project(&self, project_id: &str) -> Result<Option<ProjectRecord>> {
        use entities::project;
        Ok(project::Entity::find_by_id(project_id.to_string())
            .one(&self.db)
            .await?
            .map(project_record))
    }

    pub async fn archive_project(&self, project_id: &str) -> Result<Option<ProjectRecord>> {
        use entities::{
            agent, agent_event, agent_runtime_event, agent_runtime_snapshot, attachment,
            interaction, message, message_part, project, session, session_runtime_snapshot,
            session_skill, studio_event, studio_message, tool_approval, turn,
        };
        let Some(project) = project::Entity::find_by_id(project_id.to_string())
            .one(&self.db)
            .await?
        else {
            return Ok(None);
        };
        let tx = self.db.begin().await?;
        let session_ids = session::Entity::find()
            .filter(session::Column::ProjectId.eq(project_id.to_string()))
            .all(&tx)
            .await?
            .into_iter()
            .map(|session| session.id)
            .collect::<Vec<_>>();

        for session_id in &session_ids {
            let session_id = session_id.to_string();
            studio_event::Entity::delete_many()
                .filter(studio_event::Column::SessionId.eq(session_id.clone()))
                .exec(&tx)
                .await?;
            message_part::Entity::delete_many()
                .filter(message_part::Column::SessionId.eq(session_id.clone()))
                .exec(&tx)
                .await?;
            studio_message::Entity::delete_many()
                .filter(studio_message::Column::SessionId.eq(session_id.clone()))
                .exec(&tx)
                .await?;
            turn::Entity::delete_many()
                .filter(turn::Column::SessionId.eq(session_id.clone()))
                .exec(&tx)
                .await?;
            attachment::Entity::delete_many()
                .filter(attachment::Column::SessionId.eq(session_id.clone()))
                .exec(&tx)
                .await?;
            message::Entity::delete_many()
                .filter(message::Column::SessionId.eq(session_id.clone()))
                .exec(&tx)
                .await?;
            tool_approval::Entity::delete_many()
                .filter(tool_approval::Column::SessionId.eq(session_id.clone()))
                .exec(&tx)
                .await?;
            interaction::Entity::delete_many()
                .filter(interaction::Column::SessionId.eq(session_id.clone()))
                .exec(&tx)
                .await?;
            session_skill::Entity::delete_many()
                .filter(session_skill::Column::SessionId.eq(session_id.clone()))
                .exec(&tx)
                .await?;
            agent::Entity::delete_many()
                .filter(agent::Column::SessionId.eq(session_id.clone()))
                .exec(&tx)
                .await?;
            agent_event::Entity::delete_many()
                .filter(agent_event::Column::SessionId.eq(session_id.clone()))
                .exec(&tx)
                .await?;
            agent_runtime_event::Entity::delete_many()
                .filter(agent_runtime_event::Column::SessionId.eq(session_id.clone()))
                .exec(&tx)
                .await?;
            agent_runtime_snapshot::Entity::delete_many()
                .filter(agent_runtime_snapshot::Column::SessionId.eq(session_id.clone()))
                .exec(&tx)
                .await?;
            session_runtime_snapshot::Entity::delete_many()
                .filter(session_runtime_snapshot::Column::SessionId.eq(session_id))
                .exec(&tx)
                .await?;
        }
        session::Entity::delete_many()
            .filter(session::Column::ProjectId.eq(project_id.to_string()))
            .exec(&tx)
            .await?;

        let mut active: project::ActiveModel = project.into();
        active.updated_at = Set(unix_seconds());
        active.closed = Set(1);
        let model = active.update(&tx).await?;
        tx.commit().await?;
        Ok(Some(project_record(model)))
    }
}
