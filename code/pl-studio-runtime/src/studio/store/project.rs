use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

use anyhow::{Context, Result};
use sea_orm::sqlx::sqlite::{SqliteJournalMode, SqliteSynchronous};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectOptions, ConnectionTrait, Database,
    DatabaseBackend, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, Statement,
    TransactionTrait,
};

use crate::studio::entity as entities;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::mappers::project_record;
use crate::studio::paths::{
    default_attachments_dir, default_db_path, legacy_db_paths, project_name, sqlite_url,
};
use crate::studio::records::ProjectRecord;
use crate::studio::store::{StudioDatabaseError, StudioStore};
use crate::studio::store_support::{STUDIO_DATABASE_SCHEMA_VERSION, initialize_studio_schema};

impl StudioStore {
    pub async fn default_app() -> Result<Self> {
        let path = default_db_path()?;
        Self::open_database(&path, legacy_db_paths()?, Some(default_attachments_dir()?)).await
    }

    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let attachments = path.parent().map(|parent| parent.join("attachments"));
        Self::open_database(&path, vec![path.clone()], attachments).await
    }

    pub async fn open_memory() -> Result<Self> {
        let db = connect_sqlite(
            "sqlite::memory:",
            SqliteSynchronous::Normal,
            /* max_connections */ 1,
        )
        .await?;
        initialize_studio_schema(&db).await?;
        Ok(Self { db })
    }

    pub(super) async fn open_database(
        path: &Path,
        legacy_paths: Vec<PathBuf>,
        legacy_attachments: Option<PathBuf>,
    ) -> Result<Self> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let mut exists = tokio::fs::try_exists(path).await?;
        if exists {
            let version = read_sqlite_user_version(path).await?;
            if version > STUDIO_DATABASE_SCHEMA_VERSION && !is_legacy_database_path(path) {
                return Err(StudioDatabaseError::UnsupportedSchema {
                    found: version,
                    supported: STUDIO_DATABASE_SCHEMA_VERSION,
                }
                .into());
            }
            if version != STUDIO_DATABASE_SCHEMA_VERSION {
                archive_legacy_storage(path, vec![path.to_path_buf()], legacy_attachments).await?;
                exists = false;
            }
        } else {
            let existing_legacy = existing_storage_paths(&legacy_paths).await?;
            if !existing_legacy.is_empty() {
                archive_legacy_storage(path, existing_legacy, legacy_attachments).await?;
            }
        }

        let db = connect_sqlite(
            &sqlite_url(path),
            SqliteSynchronous::Full,
            /* max_connections */ 1,
        )
        .await?;
        if !exists {
            initialize_studio_schema(&db).await?;
        } else {
            validate_database(&db, STUDIO_DATABASE_SCHEMA_VERSION).await?;
        }
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
        self.quarantine_project(project_id).await
    }

    pub(crate) async fn quarantine_project(
        &self,
        project_id: &str,
    ) -> Result<Option<ProjectRecord>> {
        use entities::{project, thread};
        let Some(project) = project::Entity::find_by_id(project_id.to_string())
            .one(&self.db)
            .await?
        else {
            return Ok(None);
        };
        let tx = self.db.begin().await?;
        let threads = thread::Entity::find()
            .filter(thread::Column::ProjectId.eq(project_id.to_string()))
            .all(&tx)
            .await?;
        for thread in threads {
            let mut active: thread::ActiveModel = thread.into();
            active.archived = Set(1);
            active.updated_at = Set(unix_seconds());
            active.update(&tx).await?;
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
    let database = connect_read_only_database(path).await?;
    let version = database_schema_version(&database).await?;
    database.close().await?;
    Ok(version)
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

async fn validate_database(db: &DatabaseConnection, supported_version: i64) -> Result<()> {
    let version = database_schema_version(db).await?;
    if version != supported_version {
        return Err(StudioDatabaseError::UnsupportedSchema {
            found: version,
            supported: supported_version,
        }
        .into());
    }
    let row = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Sqlite,
            "PRAGMA quick_check".to_string(),
        ))
        .await?
        .ok_or_else(|| StudioDatabaseError::CorruptDatabase {
            reason: "SQLite 未返回 quick_check 结果".to_string(),
        })?;
    let result: String = row.try_get("", "quick_check")?;
    if result != "ok" {
        return Err(StudioDatabaseError::CorruptDatabase { reason: result }.into());
    }
    Ok(())
}

fn is_legacy_database_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            matches!(
                name,
                "studio_state.sqlite" | "studio_history.sqlite" | "studio_2.sqlite"
            )
        })
}

async fn existing_storage_paths(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut existing = Vec::new();
    for path in paths {
        if storage_family_exists(path).await? && !existing.contains(path) {
            existing.push(path.clone());
        }
    }
    Ok(existing)
}

async fn storage_family_exists(path: &Path) -> Result<bool> {
    if tokio::fs::try_exists(path).await? {
        return Ok(true);
    }
    for suffix in ["-wal", "-shm"] {
        let sidecar = PathBuf::from(format!("{}{suffix}", path.display()));
        if tokio::fs::try_exists(sidecar).await? {
            return Ok(true);
        }
    }
    Ok(false)
}

async fn archive_legacy_storage(
    canonical_path: &Path,
    database_paths: Vec<PathBuf>,
    attachments_path: Option<PathBuf>,
) -> Result<PathBuf> {
    let archive_dir = next_legacy_archive_dir(canonical_path).await?;
    let mut moves = Vec::new();
    let mut databases = Vec::new();
    let mut inventory = LegacyManifestInventory::default();
    for path in database_paths {
        if tokio::fs::try_exists(&path).await? {
            let version = read_sqlite_user_version(&path).await?;
            let file_name = path
                .file_name()
                .ok_or_else(|| anyhow::anyhow!("legacy Studio database path has no file name"))?;
            moves.push((path.clone(), archive_dir.join(file_name)));
            databases.push(serde_json::json!({
                "file": file_name.to_string_lossy(),
                "userVersion": version,
            }));
            inventory.extend(read_legacy_manifest_inventory(&path).await?);
        }
        for suffix in ["-wal", "-shm"] {
            let source = PathBuf::from(format!("{}{suffix}", path.display()));
            if tokio::fs::try_exists(&source).await? {
                let destination = archive_dir.join(
                    source
                        .file_name()
                        .ok_or_else(|| anyhow::anyhow!("legacy sidecar path has no file name"))?,
                );
                moves.push((source, destination));
            }
        }
    }
    if let Some(attachments_path) = attachments_path
        && tokio::fs::try_exists(&attachments_path).await?
    {
        moves.push((attachments_path, archive_dir.join("attachments")));
    }

    let external_resources = inspect_legacy_external_resources(&inventory).await;
    let manifest_path = archive_dir.join("manifest.json");
    let manifest = serde_json::json!({
        "schemaVersion": 1,
        "archivedAt": unix_seconds(),
        "databases": databases,
        "legacyRecords": inventory.as_json(),
        "externalResources": external_resources,
        "note": "Legacy Task worktrees and branches were recorded only; no external resources were deleted.",
    });
    tokio::fs::create_dir_all(&archive_dir).await?;
    if let Err(error) =
        tokio::fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest)?).await
    {
        let _ = tokio::fs::remove_dir(&archive_dir).await;
        return Err(error).context("无法写入旧版 Studio 数据归档清单");
    }

    move_archive_entries(moves, &manifest_path, &archive_dir).await?;
    Ok(archive_dir)
}

pub(super) async fn move_archive_entries(
    moves: Vec<(PathBuf, PathBuf)>,
    manifest_path: &Path,
    archive_dir: &Path,
) -> Result<()> {
    if moves.iter().any(|(_, destination)| destination.exists()) {
        let _ = tokio::fs::remove_file(manifest_path).await;
        let _ = tokio::fs::remove_dir(archive_dir).await;
        anyhow::bail!("Studio 数据库归档目标已存在，未修改原数据库");
    }
    let mut moved: Vec<(PathBuf, PathBuf)> = Vec::with_capacity(moves.len());
    for (source, destination) in moves {
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
            let _ = tokio::fs::remove_file(&manifest_path).await;
            let _ = tokio::fs::remove_dir(&archive_dir).await;
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
    Ok(())
}

#[derive(Default)]
struct LegacyManifestInventory {
    projects: Vec<LegacyProjectRecord>,
    task_runs: Vec<LegacyTaskRecord>,
    work_units: Vec<LegacyWorkUnitRecord>,
    branch_leases: Vec<LegacyBranchLeaseRecord>,
}

impl LegacyManifestInventory {
    fn extend(&mut self, other: Self) {
        self.projects.extend(other.projects);
        self.task_runs.extend(other.task_runs);
        self.work_units.extend(other.work_units);
        self.branch_leases.extend(other.branch_leases);
    }

    fn as_json(&self) -> serde_json::Value {
        serde_json::json!({
            "projects": self.projects.iter().map(LegacyProjectRecord::as_json).collect::<Vec<_>>(),
            "taskRuns": self.task_runs.iter().map(LegacyTaskRecord::as_json).collect::<Vec<_>>(),
            "workUnits": self.work_units.iter().map(LegacyWorkUnitRecord::as_json).collect::<Vec<_>>(),
            "branchLeases": self.branch_leases.iter().map(LegacyBranchLeaseRecord::as_json).collect::<Vec<_>>(),
        })
    }
}

struct LegacyProjectRecord {
    source_database: String,
    id: String,
    name: String,
    path: String,
    closed: i32,
}

impl LegacyProjectRecord {
    fn as_json(&self) -> serde_json::Value {
        serde_json::json!({
            "sourceDatabase": self.source_database,
            "id": self.id,
            "name": self.name,
            "path": self.path,
            "closed": self.closed != 0,
        })
    }
}

struct LegacyTaskRecord {
    source_database: String,
    id: String,
    root_thread_id: String,
    phase: String,
    workspace_root: String,
    git_common_dir: String,
    branch: String,
    base_commit: String,
    expected_head: String,
}

impl LegacyTaskRecord {
    fn as_json(&self) -> serde_json::Value {
        serde_json::json!({
            "sourceDatabase": self.source_database,
            "id": self.id,
            "rootThreadId": self.root_thread_id,
            "phase": self.phase,
            "workspaceRoot": self.workspace_root,
            "gitCommonDir": self.git_common_dir,
            "branch": self.branch,
            "baseCommit": self.base_commit,
            "expectedHead": self.expected_head,
        })
    }
}

struct LegacyWorkUnitRecord {
    source_database: String,
    id: String,
    task_run_id: String,
    status: String,
    worktree_path: String,
    branch: String,
    base_commit: String,
    worktree_disposition: String,
}

impl LegacyWorkUnitRecord {
    fn as_json(&self) -> serde_json::Value {
        serde_json::json!({
            "sourceDatabase": self.source_database,
            "id": self.id,
            "taskRunId": self.task_run_id,
            "status": self.status,
            "worktreePath": self.worktree_path,
            "branch": self.branch,
            "baseCommit": self.base_commit,
            "worktreeDisposition": self.worktree_disposition,
        })
    }
}

struct LegacyBranchLeaseRecord {
    source_database: String,
    id: String,
    task_run_id: String,
    git_common_dir: String,
    branch: String,
    expected_head: String,
}

impl LegacyBranchLeaseRecord {
    fn as_json(&self) -> serde_json::Value {
        serde_json::json!({
            "sourceDatabase": self.source_database,
            "id": self.id,
            "taskRunId": self.task_run_id,
            "gitCommonDir": self.git_common_dir,
            "branch": self.branch,
            "expectedHead": self.expected_head,
        })
    }
}

async fn read_legacy_manifest_inventory(path: &Path) -> Result<LegacyManifestInventory> {
    let database = connect_read_only_database(path).await?;
    validate_legacy_database(&database, path).await?;
    let source_database = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("legacy Studio database path has no UTF-8 file name"))?;
    let inventory = LegacyManifestInventory {
        projects: read_legacy_projects(&database, path, source_database).await?,
        task_runs: read_legacy_tasks(&database, path, source_database).await?,
        work_units: read_legacy_work_units(&database, path, source_database).await?,
        branch_leases: read_legacy_branch_leases(&database, path, source_database).await?,
    };
    database.close().await?;
    Ok(inventory)
}

async fn connect_read_only_database(path: &Path) -> Result<DatabaseConnection> {
    let normalized_path = path.to_string_lossy().replace('\\', "/");
    let mut options = ConnectOptions::new(format!("sqlite://{normalized_path}?mode=ro"));
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

async fn validate_legacy_database(db: &DatabaseConnection, path: &Path) -> Result<()> {
    let rows = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Sqlite,
            "PRAGMA quick_check".to_string(),
        ))
        .await
        .with_context(|| format!("无法检查旧版 Studio 数据库：{}", path.display()))?;
    let results = rows
        .into_iter()
        .map(|row| row.try_get::<String>("", "quick_check"))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if results.as_slice() != ["ok"] {
        anyhow::bail!(
            "旧版 Studio 数据库损坏，已保留原文件 {}：{}",
            path.display(),
            results.join("; ")
        );
    }
    Ok(())
}

async fn read_legacy_projects(
    db: &DatabaseConnection,
    path: &Path,
    source_database: &str,
) -> Result<Vec<LegacyProjectRecord>> {
    if !require_legacy_columns(db, path, "projects", &["id", "name", "path", "closed"]).await? {
        return Ok(Vec::new());
    }
    db.query_all_raw(Statement::from_string(
        DatabaseBackend::Sqlite,
        "SELECT id, name, path, closed FROM projects ORDER BY id".to_string(),
    ))
    .await?
    .into_iter()
    .map(|row| {
        Ok(LegacyProjectRecord {
            source_database: source_database.to_string(),
            id: row.try_get("", "id")?,
            name: row.try_get("", "name")?,
            path: row.try_get("", "path")?,
            closed: row.try_get("", "closed")?,
        })
    })
    .collect()
}

async fn read_legacy_tasks(
    db: &DatabaseConnection,
    path: &Path,
    source_database: &str,
) -> Result<Vec<LegacyTaskRecord>> {
    let columns = legacy_table_columns(db, "task_runs").await?;
    if columns.is_empty() {
        return Ok(Vec::new());
    }
    let identity_column = if columns.contains_key("root_thread_id") {
        "root_thread_id"
    } else {
        "session_id"
    };
    ensure_legacy_columns(
        path,
        "task_runs",
        &columns,
        &[
            "id",
            identity_column,
            "phase",
            "workspace_root",
            "git_common_dir",
            "branch",
            "base_commit",
            "expected_head",
        ],
    )?;
    let sql = format!(
        "SELECT id, {identity_column} AS root_thread_id, phase, workspace_root, git_common_dir, branch, base_commit, expected_head FROM task_runs ORDER BY id"
    );
    db.query_all_raw(Statement::from_string(DatabaseBackend::Sqlite, sql))
        .await?
        .into_iter()
        .map(|row| {
            Ok(LegacyTaskRecord {
                source_database: source_database.to_string(),
                id: row.try_get("", "id")?,
                root_thread_id: row.try_get("", "root_thread_id")?,
                phase: row.try_get("", "phase")?,
                workspace_root: row.try_get("", "workspace_root")?,
                git_common_dir: row.try_get("", "git_common_dir")?,
                branch: row.try_get("", "branch")?,
                base_commit: row.try_get("", "base_commit")?,
                expected_head: row.try_get("", "expected_head")?,
            })
        })
        .collect()
}

async fn read_legacy_work_units(
    db: &DatabaseConnection,
    path: &Path,
    source_database: &str,
) -> Result<Vec<LegacyWorkUnitRecord>> {
    if !require_legacy_columns(
        db,
        path,
        "work_units",
        &[
            "id",
            "task_run_id",
            "status",
            "worktree_path",
            "branch",
            "base_commit",
            "worktree_disposition",
        ],
    )
    .await?
    {
        return Ok(Vec::new());
    }
    db.query_all_raw(Statement::from_string(
        DatabaseBackend::Sqlite,
        "SELECT id, task_run_id, status, worktree_path, branch, base_commit, worktree_disposition FROM work_units ORDER BY id".to_string(),
    ))
    .await?
    .into_iter()
    .map(|row| {
        Ok(LegacyWorkUnitRecord {
            source_database: source_database.to_string(),
            id: row.try_get("", "id")?,
            task_run_id: row.try_get("", "task_run_id")?,
            status: row.try_get("", "status")?,
            worktree_path: row.try_get("", "worktree_path")?,
            branch: row.try_get("", "branch")?,
            base_commit: row.try_get("", "base_commit")?,
            worktree_disposition: row.try_get("", "worktree_disposition")?,
        })
    })
    .collect()
}

async fn read_legacy_branch_leases(
    db: &DatabaseConnection,
    path: &Path,
    source_database: &str,
) -> Result<Vec<LegacyBranchLeaseRecord>> {
    if !require_legacy_columns(
        db,
        path,
        "branch_leases",
        &[
            "id",
            "task_run_id",
            "git_common_dir",
            "branch",
            "expected_head",
        ],
    )
    .await?
    {
        return Ok(Vec::new());
    }
    db.query_all_raw(Statement::from_string(
        DatabaseBackend::Sqlite,
        "SELECT id, task_run_id, git_common_dir, branch, expected_head FROM branch_leases ORDER BY id".to_string(),
    ))
    .await?
    .into_iter()
    .map(|row| {
        Ok(LegacyBranchLeaseRecord {
            source_database: source_database.to_string(),
            id: row.try_get("", "id")?,
            task_run_id: row.try_get("", "task_run_id")?,
            git_common_dir: row.try_get("", "git_common_dir")?,
            branch: row.try_get("", "branch")?,
            expected_head: row.try_get("", "expected_head")?,
        })
    })
    .collect()
}

async fn require_legacy_columns(
    db: &DatabaseConnection,
    path: &Path,
    table: &str,
    required: &[&str],
) -> Result<bool> {
    let columns = legacy_table_columns(db, table).await?;
    if columns.is_empty() {
        return Ok(false);
    }
    ensure_legacy_columns(path, table, &columns, required)?;
    Ok(true)
}

fn ensure_legacy_columns(
    path: &Path,
    table: &str,
    columns: &HashMap<String, ()>,
    required: &[&str],
) -> Result<()> {
    let missing = required
        .iter()
        .filter(|column| !columns.contains_key(**column))
        .copied()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        anyhow::bail!(
            "旧版 Studio 数据库 {} 的 {table} 表缺少归档清单所需字段：{}；已保留原文件",
            path.display(),
            missing.join(", ")
        );
    }
    Ok(())
}

async fn legacy_table_columns(db: &DatabaseConnection, table: &str) -> Result<HashMap<String, ()>> {
    let sql = format!("PRAGMA table_info(\"{table}\")");
    db.query_all_raw(Statement::from_string(DatabaseBackend::Sqlite, sql))
        .await?
        .into_iter()
        .map(|row| Ok((row.try_get::<String>("", "name")?, ())))
        .collect()
}

struct LegacyExternalResourceSpec {
    kind: &'static str,
    source_database: String,
    task_run_id: String,
    work_unit_id: Option<String>,
    path: Option<PathBuf>,
    branch: String,
    base_commit: Option<String>,
    expected_head: Option<String>,
}

#[derive(Default)]
struct LegacyGitProbe {
    actual_branch: Option<String>,
    actual_head: Option<String>,
    dirty: Option<bool>,
    ahead_by: Option<u64>,
    errors: Vec<String>,
}

async fn inspect_legacy_external_resources(
    inventory: &LegacyManifestInventory,
) -> Vec<serde_json::Value> {
    let mut specs = inventory
        .task_runs
        .iter()
        .map(|task| LegacyExternalResourceSpec {
            kind: "taskWorkspace",
            source_database: task.source_database.clone(),
            task_run_id: task.id.clone(),
            work_unit_id: None,
            path: Some(PathBuf::from(&task.workspace_root)),
            branch: task.branch.clone(),
            base_commit: Some(task.base_commit.clone()),
            expected_head: Some(task.expected_head.clone()),
        })
        .collect::<Vec<_>>();
    specs.extend(
        inventory
            .work_units
            .iter()
            .map(|work_unit| LegacyExternalResourceSpec {
                kind: "taskWorktree",
                source_database: work_unit.source_database.clone(),
                task_run_id: work_unit.task_run_id.clone(),
                work_unit_id: Some(work_unit.id.clone()),
                path: Some(PathBuf::from(&work_unit.worktree_path)),
                branch: work_unit.branch.clone(),
                base_commit: Some(work_unit.base_commit.clone()),
                expected_head: None,
            }),
    );
    specs.extend(inventory.branch_leases.iter().map(|lease| {
        let task = inventory.task_runs.iter().find(|task| {
            task.source_database == lease.source_database && task.id == lease.task_run_id
        });
        LegacyExternalResourceSpec {
            kind: "branchLease",
            source_database: lease.source_database.clone(),
            task_run_id: lease.task_run_id.clone(),
            work_unit_id: None,
            path: task.map(|task| PathBuf::from(&task.workspace_root)),
            branch: lease.branch.clone(),
            base_commit: task.map(|task| task.base_commit.clone()),
            expected_head: Some(lease.expected_head.clone()),
        }
    }));

    let mut resources = Vec::with_capacity(specs.len());
    for spec in specs {
        resources.push(inspect_legacy_external_resource(spec).await);
    }
    resources
}

async fn inspect_legacy_external_resource(spec: LegacyExternalResourceSpec) -> serde_json::Value {
    let probe = if let Some(path) = spec.path.clone() {
        let comparison_base = spec.base_commit.clone();
        match tokio::task::spawn_blocking(move || {
            inspect_legacy_git(&path, comparison_base.as_deref())
        })
        .await
        {
            Ok(probe) => probe,
            Err(error) => LegacyGitProbe {
                errors: vec![format!("Git 只读检查任务失败：{error}")],
                ..LegacyGitProbe::default()
            },
        }
    } else {
        LegacyGitProbe {
            errors: vec!["找不到 BranchLease 对应的 Task workspace".to_string()],
            ..LegacyGitProbe::default()
        }
    };
    let matches_expected_head = spec
        .expected_head
        .as_ref()
        .zip(probe.actual_head.as_ref())
        .map(|(expected, actual)| expected == actual);
    let probe_error = (!probe.errors.is_empty()).then(|| probe.errors.join("; "));
    serde_json::json!({
        "kind": spec.kind,
        "sourceDatabase": spec.source_database,
        "taskRunId": spec.task_run_id,
        "workUnitId": spec.work_unit_id,
        "path": spec.path.map(|path| path.to_string_lossy().to_string()),
        "branch": spec.branch,
        "baseCommit": spec.base_commit,
        "expectedHead": spec.expected_head,
        "actualBranch": probe.actual_branch,
        "actualHead": probe.actual_head,
        "matchesExpectedHead": matches_expected_head,
        "dirty": probe.dirty,
        "aheadBy": probe.ahead_by,
        "probeError": probe_error,
    })
}

fn inspect_legacy_git(path: &Path, comparison_base: Option<&str>) -> LegacyGitProbe {
    let mut probe = LegacyGitProbe::default();
    if !path.is_dir() {
        probe
            .errors
            .push(format!("worktree 路径不存在：{}", path.display()));
        return probe;
    }
    match legacy_git_output(path, &["rev-parse", "HEAD"]) {
        Ok(head) => probe.actual_head = Some(head),
        Err(error) => probe.errors.push(error.to_string()),
    }
    match legacy_git_output(path, &["branch", "--show-current"]) {
        Ok(branch) if !branch.is_empty() => probe.actual_branch = Some(branch),
        Ok(_) => {}
        Err(error) => probe.errors.push(error.to_string()),
    }
    match legacy_git_output(path, &["status", "--porcelain=v1", "--untracked-files=all"]) {
        Ok(status) => probe.dirty = Some(!status.is_empty()),
        Err(error) => probe.errors.push(error.to_string()),
    }
    if let (Some(base), Some(head)) = (comparison_base, probe.actual_head.as_deref()) {
        let range = format!("{base}..{head}");
        match legacy_git_output(path, &["rev-list", "--count", &range]) {
            Ok(count) => match count.parse::<u64>() {
                Ok(count) => probe.ahead_by = Some(count),
                Err(error) => probe
                    .errors
                    .push(format!("git rev-list 返回了无效数量 `{count}`：{error}")),
            },
            Err(error) => probe.errors.push(error.to_string()),
        }
    }
    probe
}

fn legacy_git_output(path: &Path, args: &[&str]) -> Result<String> {
    let output = legacy_git_command(path, args)?;
    if !output.status.success() {
        anyhow::bail!("git {} 失败：{}", args.join(" "), git_output_error(&output));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn legacy_git_command(path: &Path, args: &[&str]) -> Result<Output> {
    let mut command = Command::new("git");
    command
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .arg("-C")
        .arg(path)
        .args(args);
    crate::process::configure_background_std_command(&mut command);
    command
        .output()
        .with_context(|| format!("无法运行 git {}", args.join(" ")))
}

fn git_output_error(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    } else {
        stderr
    }
}

async fn next_legacy_archive_dir(path: &Path) -> Result<PathBuf> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("legacy Studio database path has no parent"))?;
    let base = parent
        .join("archive")
        .join(format!("storage-legacy-{}", unix_seconds()));
    if !tokio::fs::try_exists(&base).await? {
        return Ok(base);
    }

    for sequence in 1_u32.. {
        let candidate = parent
            .join("archive")
            .join(format!("storage-legacy-{}-{sequence}", unix_seconds()));
        if !tokio::fs::try_exists(&candidate).await? {
            return Ok(candidate);
        }
    }
    unreachable!("u32 backup sequence space must not be exhausted")
}
