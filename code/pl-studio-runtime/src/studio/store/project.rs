use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use sea_orm::sqlx::sqlite::{SqliteJournalMode, SqliteSynchronous};
use sea_orm::{
    ConnectOptions, ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, Statement,
    TransactionTrait,
};

use crate::studio::catalog::CatalogStore;
use crate::studio::paths::{StudioPaths, default_db_path, sqlite_read_only_url, sqlite_url};
use crate::studio::records::ProjectRecord;
use crate::studio::store::settings::SettingsStore;
use crate::studio::store::workspaces::WorkspaceStore;
use crate::studio::store::{StudioDatabaseError, StudioStore};
use crate::studio::store_support::{STUDIO_DATABASE_SCHEMA_VERSION, initialize_studio_schema};

/// 可原地升级到当前 schema 的上一版本；模式匹配需要 const 模式。
const UPGRADEABLE_STUDIO_DATABASE_SCHEMA_VERSION: i64 = STUDIO_DATABASE_SCHEMA_VERSION - 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExistingDatabaseState {
    Current,
    /// 上一个大版本的 v2 产品库：打开后按事务原地升级到当前 schema，保留全部数据。
    UpgradeRequired,
}

/// Whether a resolved Studio home already owns persistent state from an earlier run.
///
/// The product database is deliberately *not* part of this decision: every installation, including
/// a brand-new one, materializes `<home>/studio/studio.sqlite`, so its presence says nothing about
/// prior v2 state. The evidence is a session or migration directory, call facts,
/// attachment-draft state, or an existing canonical document. Call and draft state are usable as
/// evidence because a fresh open publishes the three canonical documents before it opens the
/// call store and before the runtime creates its draft root, so either without the documents
/// means the fact source was lost
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
        let store = Self::prepare_database(path).await?;
        store.calls.start().await;
        Ok(store)
    }

    pub(crate) async fn prepare_database(path: &Path) -> Result<Self> {
        let path = resolve_configured_database_path(path).await?;
        let database_exists = tokio::fs::try_exists(&path).await?;
        let family_exists = database_family_exists(&path).await?;
        let existing_state = if database_exists {
            Some(inspect_database(&path).await?)
        } else {
            if family_exists {
                return Err(StudioDatabaseError::CorruptDatabase {
                    reason: "orphan Studio WAL/SHM files".into(),
                }
                .into());
            }
            None
        };
        // The data root is resolved once; every session, call and attachment path comes from the
        // canonical layout instead of being re-joined by each consumer.
        let paths = StudioPaths::resolve(Some(studio_home_from_database(&path)?))?;
        // Old-version publication state lives outside this version's data root and is never read.
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
            } else if existing_state == Some(ExistingDatabaseState::UpgradeRequired) {
                // v2 内的结构升级在打开可写连接后原地完成：事务内加列并推进
                // user_version，保留全部已有数据（design/17 §17.1）。
                upgrade_product_schema(&db).await?;
            }
            // ExistingDatabaseState::Current was already verified before opening writable.
            // Old session storage remains isolated outside v2.
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
                (Ok(()), Ok(())) => Err(error).context("Studio database initialization failed"),
                (Err(close_error), Ok(())) => {
                    Err(crate::studio::startup::cleanup_error(close_error).context(error))
                }
                (Ok(()), Err(cleanup_error)) => {
                    Err(crate::studio::startup::cleanup_error(cleanup_error).context(error))
                }
                (Err(close_error), Err(cleanup_error)) => {
                    Err(crate::studio::startup::cleanup_error(close_error)
                        .context(cleanup_error)
                        .context(error))
                }
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
                    Err(close_error) => {
                        crate::studio::startup::cleanup_error(close_error).context(error)
                    }
                });
            }
        };
        let (catalog, settings, workspaces) = documents;
        let calls =
            match crate::studio::storage::calls::CallsStore::prepare(&paths.calls_database()).await
            {
                Ok(calls) => calls,
                Err(error) => {
                    db.close().await.context(
                        "failed to close product database after calls preparation failed",
                    )?;
                    return Err(error);
                }
            };
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

    pub(crate) async fn close(&self) -> Result<()> {
        let (calls, product) = tokio::join!(self.calls.close(), self.db.close_by_ref());
        calls?;
        product?;
        Ok(())
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
/// The canonical layout places the product database at `<home>/studio/v2/studio.sqlite`; a database
/// configured directly in its own directory keeps that directory as the home. Both cases resolve
/// the same relative layout for sessions, calls and attachments.
fn studio_home_from_database(database: &Path) -> Result<PathBuf> {
    let parent = database
        .parent()
        .context("Studio database path has no parent directory")?;
    if parent.file_name().is_some_and(|name| name == "v2")
        && parent
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name == "studio")
    {
        return parent
            .parent()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .context("Studio versioned data directory has no home");
    }
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
        paths.migrations_dir(),
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
    validate_directory_references(&catalog, &workspaces)
        .map_err(crate::studio::startup::data_error)?;
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

fn validate_directory_references(
    catalog: &CatalogStore,
    workspaces: &WorkspaceStore,
) -> Result<()> {
    use std::collections::{BTreeMap, BTreeSet};
    let entries = catalog.entries();
    let threads = entries
        .iter()
        .map(|entry| (entry.id.as_str(), entry))
        .collect::<BTreeMap<_, _>>();
    let projects = workspaces
        .entries()
        .into_iter()
        .map(|entry| entry.id)
        .collect::<BTreeSet<_>>();
    for entry in &entries {
        ensure!(
            projects.contains(&entry.project_id),
            "catalog references a missing project"
        );
        if let Some(parent_id) = &entry.parent_thread_id {
            let parent = threads
                .get(parent_id.as_str())
                .context("catalog references a missing parent")?;
            let root = threads
                .get(entry.root_thread_id.as_str())
                .context("catalog references a missing root")?;
            ensure!(
                parent.project_id == entry.project_id
                    && root.project_id == entry.project_id
                    && root.parent_thread_id.is_none()
                    && parent.root_thread_id == entry.root_thread_id,
                "catalog parent/root references are inconsistent"
            );
        }
    }
    let mut checked = BTreeSet::new();
    for entry in &entries {
        let mut path = BTreeSet::new();
        let mut current = entry;
        while !checked.contains(current.id.as_str()) {
            ensure!(
                path.insert(current.id.as_str()),
                "catalog parent references contain a cycle"
            );
            let Some(parent) = &current.parent_thread_id else {
                break;
            };
            current = threads
                .get(parent.as_str())
                .context("catalog parent is missing")?;
        }
        checked.extend(path);
    }
    Ok(())
}

async fn require_canonical_document(path: &Path, name: &str) -> Result<()> {
    if !tokio::fs::try_exists(path).await? {
        return Err(crate::studio::startup::data_error(anyhow::anyhow!(
            "Studio {name} is missing on an existing installation ({})",
            path.display()
        )));
    }
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
        // 上一个大版本的 v2 库可以在打开后原地升级；更早的 dev 版本仍交给启动入口。
        Ok(UPGRADEABLE_STUDIO_DATABASE_SCHEMA_VERSION) => {
            Ok(ExistingDatabaseState::UpgradeRequired)
        }
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
        (Err(error), Err(close_error)) => {
            Err(crate::studio::startup::cleanup_error(close_error).context(error))
        }
    }
}

/// Applies the v22 → v23 product schema upgrade in one transaction.
///
/// The only structural change is the nullable `threads.last_user_message_at` column: existing
/// rows keep `NULL` (no accepted user message is fabricated), and the added column lands in the
/// same position as canonical creation so the schema fingerprint keeps matching. Idempotent for a
/// database that already reached the current version.
async fn upgrade_product_schema(db: &DatabaseConnection) -> Result<()> {
    let tx = db.begin().await?;
    let version = database_schema_version(&tx).await?;
    if version == STUDIO_DATABASE_SCHEMA_VERSION {
        tx.commit().await?;
        return Ok(());
    }
    anyhow::ensure!(
        version == STUDIO_DATABASE_SCHEMA_VERSION - 1,
        "Studio database upgrade requires schema version {}, found {version}",
        STUDIO_DATABASE_SCHEMA_VERSION - 1
    );
    tx.execute_unprepared("ALTER TABLE threads ADD COLUMN last_user_message_at INTEGER")
        .await?;
    tx.execute_unprepared(&format!(
        "PRAGMA user_version = {STUDIO_DATABASE_SCHEMA_VERSION}"
    ))
    .await?;
    tx.commit().await?;
    Ok(())
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
    if actual != expected {
        return Err(StudioDatabaseError::CorruptDatabase {
            reason: "incompatible schema fingerprint".into(),
        }
        .into());
    }
    Ok(())
}

async fn database_schema_version(db: &impl sea_orm::ConnectionTrait) -> Result<i64> {
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
mod major_version_storage_tests {
    use super::*;

    #[tokio::test]
    async fn old_sessions_are_never_imported_or_recreated() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let paths = StudioPaths::resolve(Some(temp.path().to_path_buf()))?;
        let old_session = temp.path().join("sessions/old-thread/state.toml");
        let old_catalog = temp.path().join("catalog.toml");
        let old_product = temp.path().join("studio/studio.sqlite");
        let config = temp.path().join("config.toml");
        for (path, data) in [
            (&old_session, b"old checkpoint".as_slice()),
            (&old_catalog, b"old catalog".as_slice()),
            (&old_product, b"old product database".as_slice()),
            (&config, b"provider configuration".as_slice()),
        ] {
            tokio::fs::create_dir_all(path.parent().expect("test path parent")).await?;
            tokio::fs::write(path, data).await?;
        }
        let store = StudioStore::open(paths.database()).await?;
        assert!(store.catalog().entries().is_empty());
        assert!(tokio::fs::try_exists(paths.catalog_file()).await?);
        for (path, expected) in [
            (&old_session, b"old checkpoint".as_slice()),
            (&old_catalog, b"old catalog".as_slice()),
            (&old_product, b"old product database".as_slice()),
            (&config, b"provider configuration".as_slice()),
        ] {
            assert_eq!(tokio::fs::read(path).await?, expected);
        }

        tokio::fs::remove_file(paths.catalog_file()).await?;
        let error = match StudioStore::open(paths.database()).await {
            Ok(_) => anyhow::bail!("a missing v2 catalog was accepted"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("catalog.toml is missing"));
        assert_eq!(tokio::fs::read(old_catalog).await?, b"old catalog");
        Ok(())
    }
}

/// v2 内产品库结构升级（22 → 23）：原地加列、保留全部数据，不伪造用户消息时间。
#[cfg(test)]
mod v2_schema_upgrade_tests {
    use super::super::directory::{DirectoryDelta, ProjectDirectoryRecord, apply_directory_delta};
    use super::*;

    #[tokio::test]
    async fn schema_22_to_23_preserves_threads_and_keeps_last_user_message_null() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let paths = StudioPaths::resolve(Some(temp.path().to_path_buf()))?;
        let database = paths.database();
        tokio::fs::create_dir_all(database.parent().expect("database parent")).await?;
        let store = StudioStore::open(&database).await?;
        let mut delta = DirectoryDelta::upsert_project(ProjectDirectoryRecord {
            id: "project-1".into(),
            name: "Project".into(),
            path: "/tmp/project".into(),
            ssh_alias: None,
            created_at: 1,
            updated_at: 1,
            last_opened_at: None,
            closed: false,
        });
        let (thread_delta, thread) = DirectoryDelta::register_root_thread(
            "thread-old".into(),
            "project-1",
            "旧会话",
            pl_protocol::ThreadModeId::simple(),
            pl_protocol::ThreadWorkspaceMode::Local,
            "/tmp/project".into(),
        );
        delta.thread_upserts = thread_delta.thread_upserts;
        let tx = store.database().begin().await?;
        apply_directory_delta(&store, &tx, &delta).await?;
        tx.commit().await?;
        // 把刚初始化的当前库降回上一版本：删除新列即得到与 v22 逐字一致的表结构。
        store
            .database()
            .execute_unprepared("ALTER TABLE threads DROP COLUMN last_user_message_at")
            .await?;
        store
            .database()
            .execute_unprepared("PRAGMA user_version = 22")
            .await?;
        assert_eq!(
            database_schema_version(store.database()).await?,
            UPGRADEABLE_STUDIO_DATABASE_SCHEMA_VERSION
        );
        drop(store);

        // 重新打开：只读探测识别 22 → 打开可写连接后事务内原地升级并校验指纹。
        let upgraded = StudioStore::open(&database).await?;
        assert_eq!(
            database_schema_version(upgraded.database()).await?,
            STUDIO_DATABASE_SCHEMA_VERSION
        );
        let record = upgraded
            .read_thread("thread-old")
            .await?
            .expect("upgraded store kept the v22 thread row");
        assert_eq!(record.id, thread.id);
        assert_eq!(record.title, "旧会话");
        // 旧目录缺失最近用户消息时间：迁移为缺失值，不以普通更新时间伪造。
        assert_eq!(record.updated_at, thread.updated_at);
        assert_eq!(record.last_user_message_at, None);
        assert_eq!(
            upgraded
                .catalog()
                .get("thread-old")
                .map(|entry| entry.last_user_message_at),
            Some(None),
        );

        // 升级幂等：再次打开不需要任何迁移步骤。
        drop(upgraded);
        let reopened = StudioStore::open(&database).await?;
        assert!(reopened.read_thread("thread-old").await?.is_some());
        Ok(())
    }
}
