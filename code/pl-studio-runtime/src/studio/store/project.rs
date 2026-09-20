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
use crate::studio::paths::{sessions_dir_beside, sqlite_read_only_url, sqlite_url};
use crate::studio::records::ProjectRecord;
use crate::studio::session_store::SessionStores;
use crate::studio::store::{StudioDatabaseError, StudioStore};
use crate::studio::store_support::{STUDIO_DATABASE_SCHEMA_VERSION, initialize_studio_schema};
use crate::studio::workspace_declarations::{WorkspaceDeclarations, layout_marker_path};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExistingDatabaseState {
    Current,
}

impl StudioStore {
    pub async fn default_app() -> Result<Self> {
        let paths = crate::studio::paths::StudioPaths::resolve(None)?;
        Self::open_at(&paths).await
    }

    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        // Standalone/test opening: declarations sit under the database's directory.
        let path = path.as_ref();
        let config_dir = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        Self::open_database(path, config_dir).await
    }

    /// Opens the configured Studio store, placing declarations at `<home>/workspaces`.
    pub(in crate::studio) async fn open_at(
        paths: &crate::studio::paths::StudioPaths,
    ) -> Result<Self> {
        Self::open_database(&paths.database(), paths.home().to_path_buf()).await
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
            .prefix("anywork-memory-attachments-")
            .tempdir()?
            .keep();
        let workspaces_root = tempfile::Builder::new()
            .prefix("anywork-memory-workspaces-")
            .tempdir()?
            .keep();
        let workspaces_marker = workspaces_root.join("workspace-layout.json");
        Ok(Self {
            db,
            sessions: SessionStores::memory(),
            attachments_dir,
            workspaces: std::sync::Arc::new(WorkspaceDeclarations::new(
                crate::config::WorkspaceConfigStore::new(workspaces_root),
                workspaces_marker,
            )),
        })
    }

    pub(super) async fn open_database(path: &Path, config_dir: PathBuf) -> Result<Self> {
        let path = resolve_configured_database_path(path).await?;
        // The per-Thread layout replaces the legacy aggregate `sessions.sqlite`. Once
        // the coordinated startup migration publishes the layout marker, the retained
        // aggregate is recovery material and startup proceeds; without a marker the
        // aggregate still owns durable history, so startup refuses rather than bypass it.
        let legacy_sessions = path.with_file_name("sessions.sqlite");
        if crate::studio::session_layout::published_marker(&path)
            .await?
            .is_none()
            && tokio::fs::try_exists(&legacy_sessions).await?
        {
            anyhow::bail!(
                "session layout migration is required before start: legacy aggregate database {} \
                 must not be bypassed by the per-Thread session layout",
                legacy_sessions.display()
            );
        }
        let database_exists = tokio::fs::try_exists(&path).await?;
        let family_exists = database_family_exists(&path).await?;
        let existing_state = if database_exists {
            Some(inspect_database(&path).await?)
        } else {
            anyhow::ensure!(
                !family_exists,
                "orphan Studio WAL/SHM files require explicit recovery"
            );
            None
        };

        let db = connect_sqlite(
            &sqlite_url(&path),
            SqliteSynchronous::Full,
            /* max_connections */ 1,
        )
        .await?;
        let created = existing_state.is_none();
        let attachments_dir = path
            .parent()
            .context("Studio database path has no parent directory")?
            .join("attachments");
        let workspaces = std::sync::Arc::new(WorkspaceDeclarations::new(
            crate::config::WorkspaceConfigStore::new(config_dir),
            layout_marker_path(&path),
        ));
        let initialization = async {
            if created {
                initialize_studio_schema(&db).await?;
            }
            // ExistingDatabaseState::Current was already verified before opening writable.
            // Session upgrades belong to the locked pre-publication coordinator.
            validate_database(&db).await
        }
        .await;
        if let Err(error) = initialization {
            let close = db.close().await;
            let cleanup = if created {
                delete_database_family(&path).await
            } else {
                Ok(())
            };
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
        let sessions = SessionStores::for_sessions_dir(sessions_dir_beside(&path));
        Ok(Self {
            db,
            sessions,
            attachments_dir,
            workspaces,
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
            .filter(project::Column::SshAlias.is_null())
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
            ssh_alias: Set(None),
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

    /// Every Project directory row, including closed ones, for declaration export and merge.
    pub(in crate::studio) async fn list_project_directory_rows(
        &self,
    ) -> Result<Vec<ProjectRecord>> {
        use entities::project;
        let rows = project::Entity::find()
            .order_by_desc(project::Column::UpdatedAt)
            .order_by_desc(project::Column::Id)
            .all(&self.db)
            .await?;
        Ok(rows.into_iter().map(project_record).collect())
    }

    /// 聚合冷加载：按 path 找到既有 Project 行身份事实。
    pub(in crate::studio) async fn find_project_by_path(
        &self,
        path: &str,
        ssh_alias: Option<&str>,
    ) -> Result<Option<ProjectRow>> {
        use entities::project;
        let server_filter = match ssh_alias {
            Some(server_id) => project::Column::SshAlias.eq(server_id.to_string()),
            None => project::Column::SshAlias.is_null(),
        };
        Ok(project::Entity::find()
            .filter(project::Column::Path.eq(path.to_string()))
            .filter(server_filter)
            .one(&self.db)
            .await?
            .map(|model| ProjectRow {
                id: model.id,
                name: model.name,
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

    /// Cold baseline of every Project's dynamic columns, for the startup owner snapshot.
    ///
    /// This is a cold API: runtime reads consume the in-memory owner, never this table.
    pub(in crate::studio) async fn list_project_dynamic(
        &self,
    ) -> Result<std::collections::HashMap<String, ProjectDynamicRow>> {
        use entities::project;
        let rows = project::Entity::find().all(&self.db).await?;
        Ok(rows
            .into_iter()
            .map(|model| {
                (
                    model.id.clone(),
                    ProjectDynamicRow {
                        created_at: model.created_at,
                        updated_at: model.updated_at,
                        last_opened_at: model.last_opened_at,
                        closed: model.closed,
                    },
                )
            })
            .collect())
    }
}

/// Dynamic Project directory columns preserved across declaration-driven upserts.
#[derive(Debug, Clone, Copy)]
pub(in crate::studio) struct ProjectDynamicRow {
    pub(in crate::studio) created_at: i64,
    pub(in crate::studio) updated_at: i64,
    pub(in crate::studio) last_opened_at: Option<i64>,
    pub(in crate::studio) closed: i32,
}

/// `find_project_by_path` 返回的持久身份事实。
#[derive(Debug, Clone)]
pub(in crate::studio) struct ProjectRow {
    pub(in crate::studio) id: String,
    pub(in crate::studio) name: String,
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

async fn inspect_database(path: &Path) -> Result<ExistingDatabaseState> {
    let database = connect_read_only_database(path).await?;
    let version = database_schema_version(&database).await;
    let validation = match version {
        Ok(STUDIO_DATABASE_SCHEMA_VERSION) => validate_database(&database)
            .await
            .map(|()| ExistingDatabaseState::Current),
        Ok(19..=21) => Err(StudioDatabaseError::StorageMigrationRequired.into()),
        Ok(found) => Err(StudioDatabaseError::UnsupportedSchema {
            found,
            supported: STUDIO_DATABASE_SCHEMA_VERSION,
        }
        .into()),
        Err(error) => Err(error),
    };
    let close = database.close().await;
    match (validation, close) {
        (Ok(state), Ok(())) => Ok(state),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(error).context("failed to close Studio schema probe"),
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
    schema_fingerprint_where(db, "1 = 1").await
}

async fn schema_fingerprint_where(db: &DatabaseConnection, predicate: &str) -> Result<String> {
    let rows = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Sqlite,
            format!(
                "SELECT type, name, tbl_name, sql
             FROM sqlite_schema
             WHERE name NOT LIKE 'sqlite_%'
               AND type IN ('table', 'index')
               AND sql IS NOT NULL
               AND {predicate}
             ORDER BY type, name"
            ),
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
    if pl_tool::workspace::path_safety::is_link_or_reparse(&metadata) || !metadata.is_file() {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn incompatible_database_is_preserved_with_its_attachments() {
        let temp = tempfile::TempDir::new().unwrap();
        let database = temp.path().join("studio.db");
        tokio::fs::write(&database, b"incompatible database")
            .await
            .unwrap();
        let old_attachment = temp.path().join("attachments/old-thread/blob");
        tokio::fs::create_dir_all(old_attachment.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&old_attachment, b"old attachment")
            .await
            .unwrap();

        assert!(StudioStore::open(&database).await.is_err());
        assert!(old_attachment.exists());
        assert_eq!(
            tokio::fs::read(&database).await.unwrap(),
            b"incompatible database"
        );
    }

    #[tokio::test]
    async fn configured_home_places_declarations_beside_config_not_in_the_data_dir() {
        let home = tempfile::TempDir::new().unwrap();
        let paths =
            crate::studio::paths::StudioPaths::resolve(Some(home.path().to_path_buf())).unwrap();
        let store = StudioStore::open_at(&paths).await.unwrap();
        store
            .workspaces()
            .declare(&crate::config::WorkspaceDeclaration::new(
                "project-x",
                "X",
                "/tmp/x",
                None,
            ))
            .unwrap();

        assert!(home.path().join("workspaces/project-x.toml").exists());
        assert!(!home.path().join("studio/workspaces").exists());
        store.sessions().shutdown().await.unwrap();
    }
}
