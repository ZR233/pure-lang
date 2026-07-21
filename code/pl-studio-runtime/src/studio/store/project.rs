use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectOptions, ConnectionTrait, Database,
    DatabaseBackend, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, Statement,
    TransactionTrait,
};

use crate::studio::entities;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::mappers::project_record;
use crate::studio::paths::{default_db_path, project_name, sqlite_url};
use crate::studio::records::ProjectRecord;
use crate::studio::store::StudioStore;
use crate::studio::store_support::{
    STUDIO_DATABASE_SCHEMA_VERSION, configure_sqlite, initialize_schema,
};

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
        let existed = tokio::fs::try_exists(path).await?;
        let url = sqlite_url(path);
        let mut db = connect_sqlite(&url).await?;
        configure_sqlite(&db).await?;

        let requires_initialization = if existed {
            let version = database_schema_version(&db).await?;
            if version == STUDIO_DATABASE_SCHEMA_VERSION {
                false
            } else {
                db.close().await?;
                remove_database_files(path).await?;
                db = connect_sqlite(&url).await?;
                configure_sqlite(&db).await?;
                true
            }
        } else {
            true
        };
        if requires_initialization {
            initialize_schema(&db).await?;
        }
        Ok(Self { db })
    }

    pub async fn open_memory() -> Result<Self> {
        let db = connect_sqlite("sqlite::memory:").await?;
        configure_sqlite(&db).await?;
        initialize_schema(&db).await?;
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
        use entities::{attachment, interaction, project, session, tool_approval};
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
            attachment::Entity::delete_many()
                .filter(attachment::Column::SessionId.eq(session_id.clone()))
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

            for sql in [
                "DELETE FROM agent_pending_inputs
                 WHERE agent_id IN (
                     SELECT agent_id FROM agent_runtime_sessions WHERE session_id = ?
                 )",
                "DELETE FROM agent_framework_events
                 WHERE agent_id IN (
                     SELECT agent_id FROM agent_runtime_sessions WHERE session_id = ?
                 )",
                "DELETE FROM agent_runtime_states
                 WHERE agent_id IN (
                     SELECT agent_id FROM agent_runtime_sessions WHERE session_id = ?
                 )",
                "DELETE FROM agent_turns WHERE session_id = ?",
                "DELETE FROM agent_runtime_traces WHERE session_id = ?",
                "DELETE FROM session_event_journal WHERE session_id = ?",
                "DELETE FROM session_view_snapshots WHERE session_id = ?",
                "DELETE FROM agent_runtime_sessions WHERE session_id = ?",
            ] {
                tx.execute(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    sql,
                    [session_id.clone().into()],
                ))
                .await?;
            }
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

async fn connect_sqlite(url: &str) -> Result<DatabaseConnection> {
    let mut options = ConnectOptions::new(url.to_string());
    options
        .max_connections(1)
        .min_connections(1)
        .connect_timeout(Duration::from_secs(8))
        .acquire_timeout(Duration::from_secs(8))
        .sqlx_logging(false);
    Ok(Database::connect(options).await?)
}

async fn database_schema_version(db: &DatabaseConnection) -> Result<i64> {
    let row = db
        .query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            "PRAGMA user_version".to_string(),
        ))
        .await?
        .ok_or_else(|| anyhow::anyhow!("SQLite 未返回 user_version"))?;
    Ok(row.try_get("", "user_version")?)
}

async fn remove_database_files(path: &Path) -> Result<()> {
    for candidate in [
        path.to_path_buf(),
        sqlite_sidecar_path(path, "-wal"),
        sqlite_sidecar_path(path, "-shm"),
    ] {
        match tokio::fs::remove_file(candidate).await {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn sqlite_sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}
