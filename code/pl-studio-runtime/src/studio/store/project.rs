use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use sea_orm::sea_query::OnConflict;
use sea_orm::sqlx::sqlite::{SqliteJournalMode, SqliteSynchronous};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectOptions, ConnectionTrait, Database,
    DatabaseBackend, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, Statement,
    TransactionTrait,
};
use tokio::io::AsyncReadExt;

use crate::studio::entity as entities;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::mappers::project_record;
use crate::studio::paths::{
    default_history_db_path, default_state_db_path, legacy_db_path, project_name, sqlite_url,
};
use crate::studio::records::ProjectRecord;
use crate::studio::store::{StudioDatabaseError, StudioStore};
use crate::studio::store_support::{
    HISTORY_DATABASE_KIND, HISTORY_DATABASE_SCHEMA_VERSION, STATE_DATABASE_KIND,
    STATE_DATABASE_SCHEMA_VERSION, initialize_history_schema, initialize_state_schema,
};

impl StudioStore {
    pub async fn default_app() -> Result<Self> {
        Self::open_pair(
            default_state_db_path()?,
            default_history_db_path()?,
            Some(legacy_db_path()?),
        )
        .await
    }

    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        let state_path = path.as_ref().to_path_buf();
        let history_path = derived_history_path(&state_path);
        Self::open_pair(&state_path, history_path, Some(state_path.clone())).await
    }

    pub async fn open_memory() -> Result<Self> {
        let db = connect_sqlite(
            "sqlite::memory:",
            SqliteSynchronous::Normal,
            /* max_connections */ 1,
        )
        .await?;
        let history_db = connect_sqlite(
            "sqlite::memory:",
            SqliteSynchronous::Full,
            /* max_connections */ 1,
        )
        .await?;
        let history_writer_db = history_db.clone();
        let generation = new_id("storage-generation");
        let created_at = unix_seconds();
        initialize_state_schema(&db, &generation, created_at).await?;
        initialize_history_schema(&history_db, &generation, created_at).await?;
        let store = Self {
            db,
            history_db,
            history_writer_db,
        };
        store.spawn_history_gc();
        Ok(store)
    }

    async fn open_pair(
        state_path: impl AsRef<Path>,
        history_path: impl AsRef<Path>,
        legacy_path: Option<PathBuf>,
    ) -> Result<Self> {
        let state_path = state_path.as_ref();
        let history_path = history_path.as_ref();
        if let Some(parent) = state_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        if let Some(parent) = history_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let mut state_exists = tokio::fs::try_exists(state_path).await?;
        let history_exists = tokio::fs::try_exists(history_path).await?;
        if let Some(legacy_path) = legacy_path.as_deref() {
            let legacy_exists = tokio::fs::try_exists(legacy_path).await?;
            let legacy_is_state = legacy_path == state_path;
            if legacy_exists
                && ((!state_exists && !history_exists) || (legacy_is_state && !history_exists))
            {
                let version = read_sqlite_user_version(legacy_path).await?;
                if version > 10 {
                    if legacy_is_state && version > STATE_DATABASE_SCHEMA_VERSION {
                        return Err(StudioDatabaseError::UnsupportedSchema {
                            found: version,
                            supported: STATE_DATABASE_SCHEMA_VERSION,
                        }
                        .into());
                    }
                    if !legacy_is_state {
                        return Err(StudioDatabaseError::UnsupportedSchema {
                            found: version,
                            supported: 10,
                        }
                        .into());
                    }
                } else {
                    let archive = archive_legacy_database(legacy_path, version).await?;
                    tracing::info!(
                        archive_name = archive
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or("storage-v10-archive"),
                        version,
                        "archived legacy Studio database before creating the dual-store generation"
                    );
                    state_exists = tokio::fs::try_exists(state_path).await?;
                }
            }
        }

        let history_exists = tokio::fs::try_exists(history_path).await?;
        if state_exists != history_exists {
            return Err(StudioDatabaseError::IncompleteDatabasePair {
                state_exists,
                history_exists,
            }
            .into());
        }

        let db = connect_sqlite(
            &sqlite_url(state_path),
            SqliteSynchronous::Normal,
            /* max_connections */ 4,
        )
        .await?;
        let history_db = connect_sqlite(
            &sqlite_url(history_path),
            SqliteSynchronous::Full,
            /* max_connections */ 3,
        )
        .await?;
        let history_writer_db = connect_sqlite(
            &sqlite_url(history_path),
            SqliteSynchronous::Full,
            /* max_connections */ 1,
        )
        .await?;
        if !state_exists {
            let generation = new_id("storage-generation");
            let created_at = unix_seconds();
            initialize_state_schema(&db, &generation, created_at).await?;
            initialize_history_schema(&history_writer_db, &generation, created_at).await?;
        } else {
            let state_generation =
                validate_database(&db, STATE_DATABASE_KIND, STATE_DATABASE_SCHEMA_VERSION).await?;
            let history_generation = validate_database(
                &history_db,
                HISTORY_DATABASE_KIND,
                HISTORY_DATABASE_SCHEMA_VERSION,
            )
            .await?;
            if state_generation != history_generation {
                return Err(StudioDatabaseError::GenerationMismatch {
                    state_generation,
                    history_generation,
                }
                .into());
            }
        }
        let store = Self {
            db,
            history_db,
            history_writer_db,
        };
        store.spawn_history_gc();
        Ok(store)
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
        use entities::{
            agent_active_input, agent_pending_input, agent_runtime_session, agent_runtime_state,
            agent_turn, attachment, history_gc_job, interaction, project, session,
            session_view_snapshot,
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
            history_gc_job::Entity::insert(history_gc_job::ActiveModel {
                id: Set(new_id("history-gc")),
                session_id: Set(session_id.clone()),
                requested_at: Set(unix_seconds()),
            })
            .on_conflict(
                OnConflict::column(history_gc_job::Column::SessionId)
                    .do_nothing()
                    .to_owned(),
            )
            .exec(&tx)
            .await?;
            attachment::Entity::delete_many()
                .filter(attachment::Column::SessionId.eq(session_id.clone()))
                .exec(&tx)
                .await?;
            interaction::Entity::delete_many()
                .filter(interaction::Column::SessionId.eq(session_id.clone()))
                .exec(&tx)
                .await?;
            let agent_ids = agent_runtime_session::Entity::find()
                .filter(agent_runtime_session::Column::SessionId.eq(session_id.clone()))
                .all(&tx)
                .await?
                .into_iter()
                .map(|claim| claim.agent_id)
                .collect::<Vec<_>>();
            if !agent_ids.is_empty() {
                agent_active_input::Entity::delete_many()
                    .filter(agent_active_input::Column::AgentId.is_in(agent_ids.clone()))
                    .exec(&tx)
                    .await?;
                agent_pending_input::Entity::delete_many()
                    .filter(agent_pending_input::Column::AgentId.is_in(agent_ids.clone()))
                    .exec(&tx)
                    .await?;
                agent_runtime_state::Entity::delete_many()
                    .filter(agent_runtime_state::Column::AgentId.is_in(agent_ids))
                    .exec(&tx)
                    .await?;
            }
            agent_turn::Entity::delete_many()
                .filter(agent_turn::Column::SessionId.eq(session_id.clone()))
                .exec(&tx)
                .await?;
            session_view_snapshot::Entity::delete_by_id(session_id.clone())
                .exec(&tx)
                .await?;
            agent_runtime_session::Entity::delete_many()
                .filter(agent_runtime_session::Column::SessionId.eq(session_id))
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
        self.spawn_history_gc();
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

async fn connect_sqlite(
    url: &str,
    synchronous: SqliteSynchronous,
    max_connections: u32,
) -> Result<DatabaseConnection> {
    let mut options = ConnectOptions::new(url.to_string());
    options
        .max_connections(max_connections)
        .min_connections(1)
        .connect_timeout(Duration::from_secs(8))
        .acquire_timeout(Duration::from_secs(8))
        .map_sqlx_sqlite_opts(move |options| {
            options
                .journal_mode(SqliteJournalMode::Wal)
                .synchronous(synchronous)
                .busy_timeout(Duration::from_secs(5))
                .foreign_keys(true)
        })
        .sqlx_logging(false);
    Ok(Database::connect(options).await?)
}

async fn read_sqlite_user_version(path: &Path) -> Result<i64> {
    const SQLITE_HEADER_LEN: usize = 64;
    const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";

    let mut file = tokio::fs::File::open(path).await?;
    let mut header = [0_u8; SQLITE_HEADER_LEN];
    file.read_exact(&mut header)
        .await
        .with_context(|| format!("无法读取 Studio SQLite header：{}", path.display()))?;
    if &header[..SQLITE_MAGIC.len()] != SQLITE_MAGIC {
        anyhow::bail!(
            "Studio 数据库 header 损坏，已保留原文件：{}",
            path.display()
        );
    }
    Ok(i64::from(u32::from_be_bytes([
        header[60], header[61], header[62], header[63],
    ])))
}

async fn database_schema_version(db: &DatabaseConnection) -> Result<i64> {
    let row = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Sqlite,
            "PRAGMA user_version".to_string(),
        ))
        .await?
        .ok_or_else(|| anyhow::anyhow!("SQLite 未返回 user_version"))?;
    Ok(row.try_get("", "user_version")?)
}

async fn validate_database(
    db: &DatabaseConnection,
    expected_kind: &str,
    supported_version: i64,
) -> Result<String> {
    let version = database_schema_version(db).await?;
    if version != supported_version {
        return Err(StudioDatabaseError::UnsupportedSchema {
            found: version,
            supported: supported_version,
        }
        .into());
    }
    let metadata = entities::storage_metadata::Entity::find_by_id("primary".to_string())
        .one(db)
        .await?
        .ok_or(StudioDatabaseError::MissingStorageMetadata)?;
    if metadata.database_kind != expected_kind || metadata.schema_version != supported_version {
        return Err(StudioDatabaseError::StorageMetadataMismatch {
            expected_kind: expected_kind.to_string(),
            found_kind: metadata.database_kind,
            expected_version: supported_version,
            found_version: metadata.schema_version,
        }
        .into());
    }
    Ok(metadata.storage_generation_id)
}

async fn archive_legacy_database(path: &Path, version: i64) -> Result<PathBuf> {
    let archive_dir = next_legacy_archive_dir(path, version).await?;
    tokio::fs::create_dir_all(&archive_dir).await?;
    let file_name = path
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("legacy Studio database path has no file name"))?;
    let mut files = vec![(path.to_path_buf(), archive_dir.join(file_name))];
    for suffix in ["-wal", "-shm"] {
        let source = PathBuf::from(format!("{}{suffix}", path.display()));
        if tokio::fs::try_exists(&source).await? {
            let destination = archive_dir.join(
                source
                    .file_name()
                    .ok_or_else(|| anyhow::anyhow!("legacy sidecar path has no file name"))?,
            );
            files.push((source, destination));
        }
    }
    if files.iter().any(|(_, destination)| destination.exists()) {
        anyhow::bail!("Studio 数据库归档目标已存在，未修改原数据库");
    }

    let mut moved: Vec<(PathBuf, PathBuf)> = Vec::with_capacity(files.len());
    for (source, destination) in files {
        if let Err(error) = tokio::fs::rename(&source, &destination).await {
            for (moved_source, moved_destination) in moved.into_iter().rev() {
                if let Err(rollback_error) =
                    tokio::fs::rename(&moved_destination, &moved_source).await
                {
                    return Err(anyhow::anyhow!(
                        "归档 {} 失败：{error}；回滚 {} 失败：{rollback_error}",
                        source.display(),
                        moved_destination.display()
                    ));
                }
            }
            return Err(error).with_context(|| {
                format!(
                    "无法将旧版 Studio 数据库文件 {} 归档到 {}",
                    source.display(),
                    destination.display()
                )
            });
        }
        moved.push((source, destination));
    }
    Ok(archive_dir)
}

async fn next_legacy_archive_dir(path: &Path, version: i64) -> Result<PathBuf> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("legacy Studio database path has no parent"))?;
    let base = parent
        .join("archive")
        .join(format!("storage-v{version}-{}", unix_seconds()));
    if !tokio::fs::try_exists(&base).await? {
        return Ok(base);
    }

    for sequence in 1_u32.. {
        let candidate = parent
            .join("archive")
            .join(format!("storage-v{version}-{}-{sequence}", unix_seconds()));
        if !tokio::fs::try_exists(&candidate).await? {
            return Ok(candidate);
        }
    }
    unreachable!("u32 backup sequence space must not be exhausted")
}

fn derived_history_path(state_path: &Path) -> PathBuf {
    let stem = state_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("studio_state");
    state_path.with_file_name(format!("{stem}.history.sqlite"))
}
