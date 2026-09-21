use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use sea_orm::sqlx::sqlite::{SqliteJournalMode, SqliteSynchronous};
#[cfg(test)]
use sea_orm::{ActiveModelTrait, ActiveValue::Set};
#[cfg(test)]
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use sea_orm::{
    ConnectOptions, ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, Statement,
};

use crate::studio::catalog::CatalogStore;
#[cfg(test)]
use crate::studio::entity as entities;
#[cfg(test)]
use crate::studio::ids::{new_id, unix_seconds};
#[cfg(test)]
use crate::studio::mappers::project_record;
#[cfg(test)]
use crate::studio::paths::project_name;
use crate::studio::paths::{StudioPaths, default_db_path, sqlite_read_only_url, sqlite_url};
use crate::studio::records::ProjectRecord;
use crate::studio::store::settings::SettingsStore;
use crate::studio::store::workspaces::WorkspaceStore;
use crate::studio::store::{StudioDatabaseError, StudioStore};
use crate::studio::store_support::{STUDIO_DATABASE_SCHEMA_VERSION, initialize_studio_schema};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExistingDatabaseState {
    Current,
}

/// Whether a resolved Studio home already owns persistent state from an earlier run.
///
/// The product database is deliberately *not* part of this decision: every installation, including
/// a brand-new one, materializes `<home>/studio/studio.sqlite`, so its presence says nothing about
/// migrated user data. The evidence that matters is state the canonical TOML documents describe:
/// a published session layout, an unmigrated legacy session database, a started (or partially
/// published) migration, a legacy attachment root, call facts, attachment-draft state, or a
/// canonical document that already exists. Call and draft state are usable as evidence because a
/// fresh open publishes the three canonical documents before it opens the call store and before the
/// runtime creates its draft root, so either without the documents means the fact source was lost
/// rather than never written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstallationState {
    /// No prior Studio state: missing documents describe a new, empty installation.
    Fresh,
    /// Prior Studio state exists: every canonical document must already be on disk.
    Existing,
}

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
        let home = tempfile::Builder::new()
            .prefix("anywork-memory-store-")
            .tempdir()?
            .keep();
        let paths = StudioPaths::resolve(Some(home))?;
        let calls =
            crate::studio::storage::calls::CallsStore::open(&paths.calls_database()).await?;
        // A brand-new in-memory store for tests starts from empty canonical documents and never
        // reads the retired product tables.
        let (catalog, settings, workspaces) =
            load_canonical_documents(&paths, InstallationState::Fresh).await?;
        let store = Self {
            db,
            calls,
            catalog,
            settings,
            workspaces,
            thread_persistence: Default::default(),
            attachment_lock: Default::default(),
            paths,
        };
        Ok(store)
    }

    pub(super) async fn open_database(path: &Path) -> Result<Self> {
        let path = resolve_configured_database_path(path).await?;
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
        // The data root is resolved once; every session, call and attachment path comes from the
        // canonical layout instead of being re-joined by each consumer.
        let paths = StudioPaths::resolve(Some(studio_home_from_database(&path)?))?;
        // The single authoritative publication gate: while a layout switch is announced but not
        // committed, no opener may classify, read or create the canonical layout. Opening here would
        // otherwise create an empty canonical `calls/calls.sqlite` / `sessions/` on a half-published
        // home, which makes publication skip the verified staged roots and drop their facts.
        crate::studio::session_migration::ensure_layout_committed(&paths).await?;
        // Decide the canonical-document contract before opening anything that creates state of its
        // own: this classification must see the home as it was left by the previous run, and every
        // store below (calls, database) materializes files on a fresh open.
        let installation = detect_installation(&paths).await?;
        let db = connect_sqlite(
            &sqlite_url(&path),
            SqliteSynchronous::Full,
            /* max_connections */ 1,
        )
        .await?;
        let created = existing_state.is_none();
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
        // Canonical TOML is the only fact source for directory and settings reads. A missing or
        // corrupt document on an installation that already owns state fails startup instead of
        // being replaced by an empty default; no retired SQLite table is read here.
        let documents = match load_canonical_documents(&paths, installation).await {
            Ok(documents) => documents,
            Err(error) => {
                let close = db.close().await;
                return Err(match close {
                    Ok(()) => error,
                    Err(close_error) => error.context(format!(
                        "canonical Studio documents failed to load; closing the database also failed: {close_error:#}"
                    )),
                });
            }
        };
        let (catalog, settings, workspaces) = documents;
        let calls =
            crate::studio::storage::calls::CallsStore::open(&paths.calls_database()).await?;
        let store = Self {
            db,
            calls,
            catalog,
            settings,
            workspaces,
            thread_persistence: Default::default(),
            attachment_lock: Default::default(),
            paths,
        };
        Ok(store)
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
            self.seed_workspace_from_model(&model).await?;
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
        self.seed_workspace_from_model(&model).await?;
        Ok(project_record(model))
    }

    #[cfg(test)]
    async fn seed_workspace_from_model(&self, model: &entities::project::Model) -> Result<()> {
        self.workspaces()
            .apply_delta(
                &crate::studio::store::directory::DirectoryDelta::upsert_project(
                    crate::studio::store::directory::ProjectDirectoryRecord {
                        id: model.id.clone(),
                        name: model.name.clone(),
                        path: model.path.clone(),
                        ssh_alias: model.ssh_alias.clone(),
                        created_at: model.created_at,
                        updated_at: model.updated_at,
                        last_opened_at: model.last_opened_at,
                        closed: model.closed != 0,
                    },
                ),
            )
            .await
    }

    pub async fn list_projects(&self) -> Result<Vec<ProjectRecord>> {
        let mut entries = self
            .workspaces()
            .entries()
            .into_iter()
            .filter(|entry| !entry.closed)
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| {
            right
                .last_opened_at
                .cmp(&left.last_opened_at)
                .then_with(|| right.updated_at.cmp(&left.updated_at))
                .then_with(|| right.id.cmp(&left.id))
        });
        Ok(entries.into_iter().map(|entry| entry.project()).collect())
    }

    /// 聚合冷加载：按 path 找到既有 Project 行身份事实。
    pub(in crate::studio) async fn find_project_by_path(
        &self,
        path: &str,
        ssh_alias: Option<&str>,
    ) -> Result<Option<ProjectRow>> {
        Ok(self
            .workspaces()
            .find_by_path(path, ssh_alias)
            .map(|entry| ProjectRow {
                id: entry.id,
                name: entry.name,
                created_at: entry.created_at,
            }))
    }

    pub async fn read_project(&self, project_id: &str) -> Result<Option<ProjectRecord>> {
        Ok(self
            .workspaces()
            .get(project_id)
            .map(|entry| entry.project()))
    }
}

/// Derives the Studio home that owns a product database file.
///
/// The canonical layout places the product database at `<home>/studio/studio.sqlite`; a database
/// configured directly in its own directory keeps that directory as the home. Both cases resolve
/// the same relative layout for sessions, calls and attachments.
fn studio_home_from_database(database: &Path) -> Result<PathBuf> {
    let parent = database
        .parent()
        .context("Studio database path has no parent directory")?;
    if parent.file_name().is_some_and(|name| name == "studio") {
        return parent
            .parent()
            .map(Path::to_path_buf)
            .context("Studio data directory has no parent");
    }
    Ok(parent.to_path_buf())
}

/// `find_project_by_path` 返回的持久身份事实。
#[derive(Debug, Clone)]
pub(in crate::studio) struct ProjectRow {
    pub(in crate::studio) id: String,
    pub(in crate::studio) name: String,
    pub(in crate::studio) created_at: i64,
}

/// Classifies the resolved Studio home before any store-owned file is created.
async fn detect_installation(paths: &StudioPaths) -> Result<InstallationState> {
    for candidate in [
        paths.sessions_dir(),
        paths.legacy_sessions_database(),
        paths.migrations_dir(),
        paths.legacy_attachments_dir(),
        paths.calls_database(),
        paths.attachment_drafts_dir(),
        paths.catalog_file(),
        paths.settings_file(),
        paths.workspaces_file(),
    ] {
        if tokio::fs::try_exists(&candidate).await? {
            return Ok(InstallationState::Existing);
        }
    }
    Ok(InstallationState::Fresh)
}

/// Loads the three canonical directory/settings documents.
///
/// A fresh installation may start from missing documents: an absent file is the empty document it
/// will create on its first write. An installation that already owns state must have every
/// canonical document on disk, because a missing one means the fact source was lost; startup fails
/// closed instead of silently substituting an empty document and masking migrated user data.
async fn load_canonical_documents(
    paths: &StudioPaths,
    installation: InstallationState,
) -> Result<(CatalogStore, SettingsStore, WorkspaceStore)> {
    if installation == InstallationState::Existing {
        require_canonical_document(&paths.catalog_file(), "catalog.toml").await?;
        require_canonical_document(&paths.settings_file(), "settings.toml").await?;
        require_canonical_document(&paths.workspaces_file(), "workspaces.toml").await?;
    }
    let catalog = CatalogStore::load(paths.catalog_file()).await?;
    let settings = SettingsStore::load(paths.settings_file()).await?;
    let workspaces = WorkspaceStore::load(paths.workspaces_file()).await?;
    if installation == InstallationState::Fresh {
        // A truly new installation starts with complete, empty canonical documents: the fact source
        // exists before the first mutation, and a later missing document is unambiguously a loss
        // instead of an as-yet unwritten file. Nothing is created when the home already owns state,
        // because that path fails closed above.
        catalog.persist_if_absent().await?;
        settings.persist_if_absent().await?;
        workspaces.persist_if_absent().await?;
    }
    Ok((catalog, settings, workspaces))
}

async fn require_canonical_document(path: &Path, name: &str) -> Result<()> {
    ensure!(
        tokio::fs::try_exists(path).await?,
        "Studio {name} is missing on an existing installation; refusing to substitute an empty \
         document and preserving existing data ({})",
        path.display()
    );
    Ok(())
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
}
