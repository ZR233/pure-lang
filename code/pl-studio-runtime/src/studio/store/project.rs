use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
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
    STUDIO_DATABASE_SCHEMA_VERSION, configure_sqlite, initialize_schema, migrate_schema,
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
            } else if version == 0 {
                if database_has_user_tables(&db).await? {
                    checkpoint_sqlite(&db).await?;
                    db.close().await?;
                    let backup = archive_incompatible_database(path).await?;
                    eprintln!(
                        "检测到不兼容的未版本化 Studio 数据库，已备份到 {} 并重建。",
                        backup.display()
                    );
                    db = connect_sqlite(&url).await?;
                    configure_sqlite(&db).await?;
                }
                true
            } else if version < STUDIO_DATABASE_SCHEMA_VERSION {
                checkpoint_sqlite(&db).await?;
                db.close().await?;
                backup_database(path, version).await?;
                db = connect_sqlite(&url).await?;
                configure_sqlite(&db).await?;
                migrate_schema(&db, version).await?;
                false
            } else {
                db.close().await?;
                anyhow::bail!(
                    "Studio 数据库版本 {version} 高于当前支持版本 \
                     {STUDIO_DATABASE_SCHEMA_VERSION}，已保留原数据库"
                );
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
                "DELETE FROM agent_active_inputs
                 WHERE agent_id IN (
                     SELECT agent_id FROM agent_runtime_sessions WHERE session_id = ?
                 )",
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

    pub(crate) async fn quarantine_project(
        &self,
        project_id: &str,
    ) -> Result<Option<ProjectRecord>> {
        use entities::{project, session, task_run};
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
            task_run::Entity::delete_many()
                .filter(task_run::Column::SessionId.eq(session_id.clone()))
                .exec(&tx)
                .await?;
            let sessions = session::Entity::find()
                .filter(session::Column::RootSessionId.eq(session_id.clone()))
                .all(&tx)
                .await?;
            for session in sessions {
                let mut active: session::ActiveModel = session.into();
                active.archived = Set(1);
                active.visibility = Set("archived".to_string());
                active.updated_at = Set(unix_seconds());
                active.update(&tx).await?;
            }
        }
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

async fn database_has_user_tables(db: &DatabaseConnection) -> Result<bool> {
    let row = db
        .query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT EXISTS(
                 SELECT 1
                 FROM sqlite_schema
                 WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
             ) AS has_user_tables"
                .to_string(),
        ))
        .await?
        .ok_or_else(|| anyhow::anyhow!("SQLite 未返回用户表检查结果"))?;
    Ok(row.try_get::<i64>("", "has_user_tables")? != 0)
}

async fn checkpoint_sqlite(db: &DatabaseConnection) -> Result<()> {
    db.execute(Statement::from_string(
        DatabaseBackend::Sqlite,
        "PRAGMA wal_checkpoint(TRUNCATE)".to_string(),
    ))
    .await?;
    Ok(())
}

async fn archive_incompatible_database(path: &Path) -> Result<PathBuf> {
    let backup = next_legacy_backup_path(path).await?;
    tokio::fs::rename(path, &backup).await.with_context(|| {
        format!(
            "无法将不兼容的 Studio 数据库 {} 归档到 {}",
            path.display(),
            backup.display()
        )
    })?;

    for suffix in ["-wal", "-shm"] {
        let sidecar = PathBuf::from(format!("{}{suffix}", path.display()));
        if tokio::fs::try_exists(&sidecar).await? {
            let backup_sidecar = PathBuf::from(format!("{}{suffix}", backup.display()));
            tokio::fs::rename(&sidecar, &backup_sidecar)
                .await
                .with_context(|| {
                    format!(
                        "无法将 SQLite sidecar {} 归档到 {}",
                        sidecar.display(),
                        backup_sidecar.display()
                    )
                })?;
        }
    }
    Ok(backup)
}

async fn next_legacy_backup_path(path: &Path) -> Result<PathBuf> {
    let base = PathBuf::from(format!("{}.legacy-v0.bak", path.display()));
    if !tokio::fs::try_exists(&base).await? {
        return Ok(base);
    }

    for sequence in 1_u32.. {
        let candidate = PathBuf::from(format!("{}.legacy-v0.{sequence}.bak", path.display()));
        if !tokio::fs::try_exists(&candidate).await? {
            return Ok(candidate);
        }
    }
    unreachable!("u32 backup sequence space must not be exhausted")
}

async fn backup_database(path: &Path, version: i64) -> Result<PathBuf> {
    let backup = PathBuf::from(format!("{}.v{version}.bak", path.display()));
    if !tokio::fs::try_exists(&backup).await? {
        tokio::fs::copy(path, &backup).await?;
    }
    Ok(backup)
}
