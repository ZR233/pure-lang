use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use sea_orm::sqlx::sqlite::{SqliteJournalMode, SqliteSynchronous};
#[cfg(test)]
use sea_orm::{ActiveModelTrait, ActiveValue::Set};
use sea_orm::{
    ColumnTrait, ConnectOptions, ConnectionTrait, Database, DatabaseBackend, DatabaseConnection,
    EntityTrait, QueryFilter, QueryOrder, Statement,
};

use crate::studio::entity as entities;
#[cfg(test)]
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::mappers::project_record;
#[cfg(test)]
use crate::studio::paths::project_name;
use crate::studio::paths::{default_db_path, sqlite_read_only_url, sqlite_url};
use crate::studio::records::ProjectRecord;
use crate::studio::store::{StudioDatabaseError, StudioStore};
use crate::studio::store_support::{STUDIO_DATABASE_SCHEMA_VERSION, initialize_studio_schema};

impl StudioStore {
    pub async fn default_app() -> Result<Self> {
        Self::open_database(&default_db_path()?).await
    }

    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_database(path.as_ref()).await
    }

    pub async fn open_memory() -> Result<Self> {
        let db = connect_sqlite(
            "sqlite::memory:",
            SqliteSynchronous::Normal,
            /* max_connections */ 1,
        )
        .await?;
        initialize_studio_schema(&db).await?;
        validate_database(&db).await?;
        let attachments_dir = tempfile::Builder::new()
            .prefix("pure-studio-memory-attachments-")
            .tempdir()?
            .keep();
        Ok(Self {
            db,
            attachments_dir,
        })
    }

    /// 缩短单连接的 SQLite 锁等待，仅供真实锁恢复测试使用。
    #[cfg(test)]
    pub(crate) async fn use_short_busy_timeout_for_test(&self) -> Result<()> {
        self.db
            .execute_unprepared("PRAGMA busy_timeout = 25")
            .await?;
        Ok(())
    }

    pub(super) async fn open_database(path: &Path) -> Result<Self> {
        let path = resolve_configured_database_path(path).await?;
        let database_exists = tokio::fs::try_exists(&path).await?;
        let family_exists = database_family_exists(&path).await?;
        // Studio schema 只支持当前 v12：其他版本直接按不兼容数据库精确重建，
        // 不保留 Task 状态或工具协议的跨版本迁移链。
        let existing_is_compatible = if database_exists {
            match inspect_database(&path).await {
                Ok(()) => true,
                Err(error) => {
                    tracing::warn!(
                        path = %path.display(),
                        error = %error,
                        "Studio database is incompatible and will be rebuilt"
                    );
                    false
                }
            }
        } else {
            false
        };

        if family_exists && !existing_is_compatible {
            delete_database_family(&path).await?;
        }

        let db = connect_sqlite(
            &sqlite_url(&path),
            SqliteSynchronous::Full,
            /* max_connections */ 1,
        )
        .await?;
        let created = !existing_is_compatible;
        let initialization = async {
            if created {
                initialize_studio_schema(&db).await?;
            }
            validate_database(&db).await
        }
        .await;
        if let Err(error) = initialization {
            let close = db.close().await;
            let cleanup = delete_database_family(&path).await;
            return match (close, cleanup) {
                (Ok(()), Ok(())) => {
                    Err(error).context("Studio database initialization failed")
                }
                (Err(close_error), Ok(())) => Err(error).context(format!(
                    "Studio database initialization failed; closing the database also failed: {close_error:#}"
                )),
                (Ok(()), Err(cleanup_error)) => Err(error).context(format!(
                    "Studio database initialization failed; partial database cleanup also failed: {cleanup_error:#}"
                )),
                (Err(close_error), Err(cleanup_error)) => Err(error).context(format!(
                    "Studio database initialization failed; closing the database failed: {close_error:#}; partial database cleanup also failed: {cleanup_error:#}"
                )),
            };
        }
        let attachments_dir = path
            .parent()
            .context("Studio database path has no parent directory")?
            .join("attachments");
        Ok(Self {
            db,
            attachments_dir,
        })
    }

    /// 测试 seed 入口：按 path 直接同步 upsert Project 行。
    ///
    /// 生产路径的打开必须经 `DirectoryDelta::upsert_project` +
    /// `ProductEventBus::commit_directory`（内存先行、异步落库）。
    #[cfg(test)]
    pub(crate) async fn upsert_project(&self, path: impl AsRef<Path>) -> Result<ProjectRecord> {
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

    /// 聚合冷加载：按 path 找到既有 Project 行身份事实。
    pub(in crate::studio) async fn find_project_by_path(
        &self,
        path: &str,
    ) -> Result<Option<ProjectRow>> {
        use entities::project;
        Ok(project::Entity::find()
            .filter(project::Column::Path.eq(path.to_string()))
            .one(&self.db)
            .await?
            .map(|model| ProjectRow {
                id: model.id,
                created_at: model.created_at,
            }))
    }

    pub async fn read_project(&self, project_id: &str) -> Result<Option<ProjectRecord>> {
        use entities::project;
        Ok(project::Entity::find_by_id(project_id.to_string())
            .one(&self.db)
            .await?
            .map(project_record))
    }
}

/// `find_project_by_path` 返回的持久身份事实。
#[derive(Debug, Clone)]
pub(in crate::studio) struct ProjectRow {
    pub(in crate::studio) id: String,
    pub(in crate::studio) created_at: i64,
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

async fn connect_read_only_database(path: &Path) -> Result<DatabaseConnection> {
    let mut options = ConnectOptions::new(sqlite_read_only_url(path));
    options
        .max_connections(1)
        .min_connections(1)
        .connect_timeout(Duration::from_secs(8))
        .acquire_timeout(Duration::from_secs(8))
        .sqlx_logging(false);
    Database::connect(options)
        .await
        .with_context(|| format!("无法以只读方式打开 Studio 数据库：{}", path.display()))
}

async fn inspect_database(path: &Path) -> Result<()> {
    let database = connect_read_only_database(path).await?;
    let validation = validate_database_version(&database, STUDIO_DATABASE_SCHEMA_VERSION).await;
    let close = database.close().await;
    match (validation, close) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(error)) => Err(error).context("failed to close Studio schema probe"),
        (Err(error), Err(close_error)) => Err(error).context(format!(
            "Studio schema probe failed; closing its connection also failed: {close_error}"
        )),
    }
}

async fn validate_database(db: &DatabaseConnection) -> Result<()> {
    validate_database_version(db, STUDIO_DATABASE_SCHEMA_VERSION).await
}

async fn validate_database_version(db: &DatabaseConnection, expected_version: i64) -> Result<()> {
    let version = database_schema_version(db).await?;
    if version != expected_version {
        return Err(StudioDatabaseError::UnsupportedSchema {
            found: version,
            supported: expected_version,
        }
        .into());
    }

    let rows = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Sqlite,
            "PRAGMA quick_check".to_string(),
        ))
        .await?;
    let results = rows
        .into_iter()
        .map(|row| row.try_get::<String>("", "quick_check"))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if results.as_slice() != ["ok"] {
        return Err(StudioDatabaseError::CorruptDatabase {
            reason: results.join("; "),
        }
        .into());
    }

    let actual = schema_fingerprint(db).await?;
    let expected = expected_schema_fingerprint().await?;
    ensure!(
        actual == expected,
        "Studio SQLite required tables, columns, indexes, or schema fingerprint are incompatible"
    );
    Ok(())
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

async fn expected_schema_fingerprint() -> Result<String> {
    let database = connect_sqlite(
        "sqlite::memory:",
        SqliteSynchronous::Normal,
        /* max_connections */ 1,
    )
    .await?;
    initialize_studio_schema(&database).await?;
    let fingerprint = schema_fingerprint(&database).await;
    database.close().await?;
    fingerprint
}

async fn schema_fingerprint(db: &DatabaseConnection) -> Result<String> {
    let rows = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT type, name, tbl_name, sql
             FROM sqlite_schema
             WHERE name NOT LIKE 'sqlite_%'
               AND type IN ('table', 'index')
               AND sql IS NOT NULL
             ORDER BY type, name"
                .to_string(),
        ))
        .await?;
    let mut entries = Vec::with_capacity(rows.len());
    for row in rows {
        let kind: String = row.try_get("", "type")?;
        let name: String = row.try_get("", "name")?;
        let table: String = row.try_get("", "tbl_name")?;
        let sql: String = row.try_get("", "sql")?;
        let normalized_sql = sql.split_whitespace().collect::<Vec<_>>().join(" ");
        entries.push(format!("{kind}:{name}:{table}:{normalized_sql}"));
    }
    Ok(format!("v1:{}", entries.join("|")))
}

async fn resolve_configured_database_path(path: &Path) -> Result<PathBuf> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let file_name = path
        .file_name()
        .context("configured Studio database path has no file name")?
        .to_os_string();
    let parent = path
        .parent()
        .context("configured Studio database path has no parent")?;
    tokio::fs::create_dir_all(parent).await?;
    let parent = std::fs::canonicalize(parent)
        .context("failed to resolve configured Studio database directory")?;
    let resolved = parent.join(file_name);
    if tokio::fs::try_exists(&resolved).await? {
        validate_database_family_member(&resolved, &parent)?;
    }
    Ok(resolved)
}

async fn database_family_exists(path: &Path) -> Result<bool> {
    for member in database_family_paths(path) {
        if tokio::fs::try_exists(member).await? {
            return Ok(true);
        }
    }
    Ok(false)
}

async fn delete_database_family(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .context("configured Studio database path has no parent")?;
    let parent = std::fs::canonicalize(parent)
        .context("failed to resolve configured Studio database directory")?;
    let members = database_family_paths(path);
    for member in &members {
        if tokio::fs::try_exists(member).await? {
            validate_database_family_member(member, &parent)?;
        }
    }
    for member in members {
        match tokio::fs::remove_file(&member).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "failed to delete incompatible Studio database file {}",
                        member.display()
                    )
                });
            }
        }
    }
    Ok(())
}

fn validate_database_family_member(path: &Path, expected_parent: &Path) -> Result<()> {
    ensure!(
        path.parent() == Some(expected_parent),
        "Studio database cleanup target escaped its configured directory"
    );
    let metadata = std::fs::symlink_metadata(path)
        .with_context(|| format!("failed to inspect Studio database file {}", path.display()))?;
    if pl_core::path_safety::is_link_or_reparse(&metadata) || !metadata.is_file() {
        bail!(
            "Studio database cleanup target is not a regular non-reparse file: {}",
            path.display()
        );
    }
    Ok(())
}

fn database_family_paths(path: &Path) -> [PathBuf; 3] {
    [
        path.to_path_buf(),
        sidecar_path(path, "-wal"),
        sidecar_path(path, "-shm"),
    ]
}

fn sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}
