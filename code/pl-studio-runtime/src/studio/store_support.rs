//! Studio product schema; configuration, projects and credential references survive session reset.
//! Session-format upgrades run only through the locked startup reset coordinator after backup.

use anyhow::{Context, Result, bail, ensure};
use sea_orm::sea_query::{Index, IndexCreateStatement, IndexOrder};
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait, Value,
};

use crate::studio::entity;

pub(super) const STUDIO_DATABASE_SCHEMA_VERSION: i64 = 22;

pub(super) async fn initialize_studio_schema(db: &DatabaseConnection) -> Result<()> {
    create_thread_lifecycle_tables(db).await?;
    db.get_schema_builder()
        .register(entity::app_setting::Entity)
        .register(entity::project::Entity)
        .register(entity::studio_object::Entity)
        .apply(db)
        .await?;
    create_state_indexes(db).await?;
    set_schema_version(db).await?;
    Ok(())
}

async fn create_thread_lifecycle_tables(db: &DatabaseConnection) -> Result<()> {
    db.execute_unprepared(
        r#"
        CREATE TABLE IF NOT EXISTS threads (
            id TEXT PRIMARY KEY NOT NULL,
            project_id TEXT NOT NULL,
            title TEXT NOT NULL,
            mode TEXT NOT NULL,
            root_thread_id TEXT NOT NULL,
            parent_thread_id TEXT,
            role TEXT NOT NULL,
            agent_path TEXT NOT NULL UNIQUE,
            state_json TEXT NOT NULL CHECK (json_valid(state_json)),
            state_kind TEXT GENERATED ALWAYS AS (
                json_extract(state_json, '$.kind')
            ) STORED NOT NULL CHECK (
                state_kind IN (
                    'idle', 'queued', 'running', 'waitingTool',
                    'waitingInteraction', 'cancelling', 'closing', 'closed',
                    'faulted'
                )
            ),
            revision INTEGER NOT NULL,
            runtime_revision INTEGER,
            event_sequence INTEGER NOT NULL,
            metadata_json TEXT NOT NULL,
            usage_json TEXT NOT NULL,
            last_context_tokens INTEGER,
            trace_sequence INTEGER NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            archived INTEGER NOT NULL,
            workspace_mode TEXT NOT NULL DEFAULT 'local',
            workspace_path TEXT NOT NULL DEFAULT '',
            FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
        );

        "#,
    )
    .await?;
    Ok(())
}

pub(super) fn non_empty_title(title: &str) -> String {
    let title = title.trim();
    if title.is_empty() {
        "新会话".to_string()
    } else {
        title.chars().take(80).collect()
    }
}

async fn set_schema_version(db: &impl ConnectionTrait) -> Result<()> {
    db.execute_unprepared(&format!(
        "PRAGMA user_version = {STUDIO_DATABASE_SCHEMA_VERSION}"
    ))
    .await?;
    Ok(())
}

async fn create_state_indexes(db: &DatabaseConnection) -> Result<()> {
    create_project_indexes(db).await?;
    let indexes = [
        Index::create()
            .name("idx_threads_project_updated")
            .table(entity::thread::Entity)
            .col(entity::thread::Column::ProjectId)
            .col(entity::thread::Column::Archived)
            .col((entity::thread::Column::UpdatedAt, IndexOrder::Desc))
            .col((entity::thread::Column::Id, IndexOrder::Desc))
            .to_owned(),
        Index::create()
            .name("idx_threads_root_parent")
            .table(entity::thread::Entity)
            .col(entity::thread::Column::RootThreadId)
            .col(entity::thread::Column::ParentThreadId)
            .col(entity::thread::Column::CreatedAt)
            .to_owned(),
    ];
    for index in indexes {
        execute_index(db, index).await?;
    }
    Ok(())
}

async fn create_project_indexes(db: &impl ConnectionTrait) -> Result<()> {
    let recent = Index::create()
        .name("idx_projects_closed_last_opened_at")
        .table(entity::project::Entity)
        .col(entity::project::Column::Closed)
        .col((entity::project::Column::LastOpenedAt, IndexOrder::Desc))
        .col((entity::project::Column::UpdatedAt, IndexOrder::Desc))
        .col((entity::project::Column::Id, IndexOrder::Desc))
        .to_owned();
    db.execute(&recent).await?;
    db.execute_unprepared(
        "CREATE UNIQUE INDEX idx_projects_local_path
         ON projects(path) WHERE ssh_alias IS NULL;
         CREATE UNIQUE INDEX idx_projects_remote_path
         ON projects(ssh_alias, path) WHERE ssh_alias IS NOT NULL;",
    )
    .await?;
    Ok(())
}

async fn execute_index(db: &DatabaseConnection, index: IndexCreateStatement) -> Result<()> {
    db.execute(&index).await?;
    Ok(())
}

/// Additive product schema upgrade for an existing Studio database.
///
/// Data-preserving and restartable: the schema version is published only after every column
/// and payload conversion in the same transaction succeeded.
pub(super) async fn upgrade_product_schema(db: &DatabaseConnection) -> Result<()> {
    let tx = db.begin().await?;
    match studio_schema_version(&tx).await? {
        20..=22 => upgrade_product_schema_in_transaction(&tx).await?,
        version => bail!("unsupported Studio schema {version}; product data preserved"),
    }
    tx.commit().await?;
    Ok(())
}

/// Applies the v20 → v21 → v22 product schema upgrades in the caller's transaction.
///
/// Each step is gated by the version it upgrades **from** and publishes `user_version`
/// only after its own column change and payload backfill succeeded, so a v20 database
/// reaches the current version through both steps in one transaction while a v21 database
/// runs only the 21 → 22 step. Every step stays idempotent and restartable.
async fn upgrade_product_schema_in_transaction(tx: &sea_orm::DatabaseTransaction) -> Result<()> {
    if studio_schema_version(tx).await? <= 20 {
        tx.execute_unprepared(
            "ALTER TABLE threads ADD COLUMN workspace_mode TEXT NOT NULL DEFAULT 'local';",
        )
        .await?;
        migrate_worktree_lease_payloads(tx).await?;
        tx.execute_unprepared("PRAGMA user_version = 21").await?;
    }
    if studio_schema_version(tx).await? <= 21 {
        tx.execute_unprepared(
            "ALTER TABLE threads ADD COLUMN workspace_path TEXT NOT NULL DEFAULT '';",
        )
        .await?;
        migrate_thread_workspace_paths(tx).await?;
        tx.execute_unprepared(&format!(
            "PRAGMA user_version = {STUDIO_DATABASE_SCHEMA_VERSION}"
        ))
        .await?;
    }
    Ok(())
}

/// Backfills `threads.workspace_path` for the v21 → v22 product migration.
///
/// `local` rows take their owning Project's canonical `path`. `worktree` rows take the
/// `path` of the durable session `worktreeLease` payload owned by their root Thread
/// (children inherit the session address); when no session lease remains the owning
/// Project's `path` is used instead. The migration never substitutes a default value for
/// a real address, and never removes Thread rows, session associations or leases.
async fn migrate_thread_workspace_paths(tx: &sea_orm::DatabaseTransaction) -> Result<()> {
    // `local` rows require an existing owning Project with a non-empty canonical path;
    // a missing Project or empty path aborts the migration (rolled back with its backup).
    let locals = tx
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT id, project_id FROM threads WHERE workspace_mode = 'local'".to_owned(),
        ))
        .await?;
    for row in locals {
        let id: String = row.try_get("", "id")?;
        let project_id: String = row.try_get("", "project_id")?;
        let path = project_path_for_thread(tx, &project_id, &id).await?;
        tx.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE threads SET workspace_path = ? WHERE id = ?",
            [Value::String(Some(path)), Value::String(Some(id))],
        ))
        .await?;
    }
    let rows = tx
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT id, root_thread_id, project_id FROM threads WHERE workspace_mode = 'worktree'"
                .to_owned(),
        ))
        .await?;
    for row in rows {
        let id: String = row.try_get("", "id")?;
        let root_thread_id: String = row.try_get("", "root_thread_id")?;
        let project_id: String = row.try_get("", "project_id")?;
        let lease = tx
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "SELECT payload_json, payload_hash FROM studio_objects \
                 WHERE owner_kind = 'agent' AND owner_id = ? AND object_kind = 'worktreeLease'",
                [Value::String(Some(root_thread_id.clone()))],
            ))
            .await?;
        let workspace_path = match lease {
            Some(lease) => {
                let payload_json: String = lease.try_get("", "payload_json")?;
                let payload_hash: String = lease.try_get("", "payload_hash")?;
                ensure!(
                    pl_core::context::content_hash(payload_json.as_bytes()) == payload_hash,
                    "worktree lease payload hash mismatch for Thread {id}; data preserved"
                );
                let payload: serde_json::Value = serde_json::from_str(&payload_json)
                    .with_context(|| format!("invalid worktree lease payload for {id}"))?;
                ensure!(
                    payload.get("ownerKind").and_then(serde_json::Value::as_str) == Some("session"),
                    "worktree Thread {id} lease is not session-owned; data preserved"
                );
                ensure!(
                    payload
                        .get("ownerThreadId")
                        .and_then(serde_json::Value::as_str)
                        == Some(root_thread_id.as_str()),
                    "worktree Thread {id} lease owner does not match its root Thread; data preserved"
                );
                payload
                    .get("path")
                    .and_then(serde_json::Value::as_str)
                    .filter(|path| !path.trim().is_empty())
                    .with_context(|| {
                        format!("worktree Thread {id} lease has no path; data preserved")
                    })?
                    .to_owned()
            }
            None => project_path_for_thread(tx, &project_id, &id).await?,
        };
        tx.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE threads SET workspace_path = ? WHERE id = ?",
            [Value::String(Some(workspace_path)), Value::String(Some(id))],
        ))
        .await?;
    }
    Ok(())
}

/// Resolves the owning Project's non-empty canonical path for a Thread, or fails so the
/// migration rolls back instead of writing an empty workspace address.
async fn project_path_for_thread(
    tx: &sea_orm::DatabaseTransaction,
    project_id: &str,
    thread_id: &str,
) -> Result<String> {
    let row = tx
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT path FROM projects WHERE id = ?",
            [Value::String(Some(project_id.to_owned()))],
        ))
        .await?;
    let path = row
        .with_context(|| {
            format!("Thread {thread_id} references a missing Project {project_id}; data preserved")
        })?
        .try_get::<String>("", "path")?;
    ensure!(
        !path.trim().is_empty(),
        "Project {project_id} has an empty path; Thread {thread_id} workspace_path not backfilled (data preserved)"
    );
    Ok(path)
}

async fn studio_schema_version(db: &impl ConnectionTrait) -> Result<i64> {
    let row = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Sqlite,
            "PRAGMA user_version".to_owned(),
        ))
        .await?
        .context("missing Studio schema version")?;
    Ok(row.try_get::<i64>("", "user_version")?)
}

/// Converts version 1 worktree lease payloads (`childId`) into version 2 ownership
/// (`ownerKind` + `ownerThreadId`); the legacy decoder serves only this migration.
async fn migrate_worktree_lease_payloads(tx: &sea_orm::DatabaseTransaction) -> Result<()> {
    let rows = tx
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT owner_id, payload_json FROM studio_objects \
             WHERE owner_kind = 'agent' AND object_kind = 'worktreeLease' AND schema_version = 1"
                .to_owned(),
        ))
        .await?;
    for row in rows {
        let owner_id: String = row.try_get("", "owner_id")?;
        let payload_json: String = row.try_get("", "payload_json")?;
        let migrated = crate::studio::agent_host::worktree_lease::migrate_lease_payload_v1_to_v2(
            &payload_json,
        )
        .with_context(|| format!("unsupported worktree lease payload for {owner_id}"))?;
        let payload_hash = pl_core::context::content_hash(migrated.as_bytes());
        tx.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE studio_objects SET payload_json = ?, payload_hash = ?, schema_version = ? \
             WHERE owner_kind = 'agent' AND object_kind = 'worktreeLease' AND owner_id = ?",
            [
                Value::String(Some(migrated)),
                Value::String(Some(payload_hash)),
                Value::BigInt(Some(2)),
                Value::String(Some(owner_id)),
            ],
        ))
        .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ConnectOptions, Database, DatabaseConnection};

    /// 版本 20 的 `threads` 表（尚无 `workspace_mode`），与当时的建表语句逐字一致。
    const VERSION_20_SCHEMA: &str = "CREATE TABLE threads (
            id TEXT PRIMARY KEY NOT NULL,
            project_id TEXT NOT NULL,
            title TEXT NOT NULL,
            mode TEXT NOT NULL,
            root_thread_id TEXT NOT NULL,
            parent_thread_id TEXT,
            role TEXT NOT NULL,
            agent_path TEXT NOT NULL UNIQUE,
            state_json TEXT NOT NULL CHECK (json_valid(state_json)),
            state_kind TEXT GENERATED ALWAYS AS (
                json_extract(state_json, '$.kind')
            ) STORED NOT NULL CHECK (
                state_kind IN (
                    'idle', 'queued', 'running', 'waitingTool',
                    'waitingInteraction', 'cancelling', 'closing', 'closed',
                    'faulted'
                )
            ),
            revision INTEGER NOT NULL,
            runtime_revision INTEGER,
            event_sequence INTEGER NOT NULL,
            metadata_json TEXT NOT NULL,
            usage_json TEXT NOT NULL,
            last_context_tokens INTEGER,
            trace_sequence INTEGER NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            archived INTEGER NOT NULL,
            FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
        );
        CREATE TABLE studio_objects (
            owner_kind TEXT NOT NULL,
            owner_id TEXT NOT NULL,
            object_kind TEXT NOT NULL,
            revision INTEGER NOT NULL,
            schema_version INTEGER NOT NULL,
            payload_json TEXT NOT NULL,
            payload_hash TEXT NOT NULL,
            updated_at INTEGER NOT NULL,
            PRIMARY KEY (owner_kind, owner_id, object_kind)
        );
        CREATE TABLE projects (
            id TEXT PRIMARY KEY NOT NULL,
            name TEXT NOT NULL,
            path TEXT NOT NULL,
            ssh_server_id TEXT,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            last_opened_at INTEGER,
            closed INTEGER NOT NULL
        );
        PRAGMA user_version=20;";

    async fn open(path: &std::path::Path) -> DatabaseConnection {
        let mut options = ConnectOptions::new(format!("sqlite://{}?mode=rwc", path.display()));
        options
            .max_connections(1)
            .min_connections(1)
            .sqlx_logging(false);
        Database::connect(options).await.unwrap()
    }

    #[tokio::test]
    async fn schema_20_to_21_preserves_threads_rows_and_migrates_lease_ownership() {
        let directory = tempfile::tempdir().unwrap();
        let db = open(&directory.path().join("studio.sqlite")).await;
        db.execute_unprepared(VERSION_20_SCHEMA).await.unwrap();
        db.execute_unprepared(
            "INSERT INTO projects (id, name, path, ssh_server_id, created_at, updated_at,
                last_opened_at, closed)
             VALUES ('project-1', 'Project', '/workspace', NULL, 1, 1, 1, 0);",
        )
        .await
        .unwrap();
        db.execute_unprepared(
            "INSERT INTO threads (id, project_id, title, mode, root_thread_id, parent_thread_id,
                role, agent_path, state_json, revision, runtime_revision, event_sequence,
                metadata_json, usage_json, last_context_tokens, trace_sequence, created_at,
                updated_at, archived)
             VALUES ('thread-existing', 'project-1', 'Existing', 'mode.simple', 'thread-existing',
                NULL, 'planner', 'thread-existing', '{\"kind\":\"idle\",\"error\":null}', 0, 1, 0,
                '{}', '{}', NULL, 0, 5, 7, 0);",
        )
        .await
        .unwrap();
        let legacy_payload = serde_json::json!({
            "revision": 2,
            "state": "active",
            "childId": "child-1",
            "rootThreadId": "root-1",
            "projectId": "project-1",
            "sshServerId": null,
            "repositoryRoot": "/repo",
            "path": "/repo/.anywork/worktrees/root-1/child-1",
            "branch": "pure-agent-child-1",
            "baseCommit": "base",
        })
        .to_string();
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO studio_objects (owner_kind, owner_id, object_kind, revision,
                schema_version, payload_json, payload_hash, updated_at)
             VALUES ('agent', 'child-1', 'worktreeLease', 2, 1, ?, ?, 9)",
            [
                Value::String(Some(legacy_payload.clone())),
                Value::String(Some(pl_core::context::content_hash(
                    legacy_payload.as_bytes(),
                ))),
            ],
        ))
        .await
        .unwrap();

        upgrade_product_schema(&db).await.unwrap();

        assert_eq!(studio_schema_version(&db).await.unwrap(), 22);
        let row = db
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT title, workspace_mode, workspace_path FROM threads WHERE id = 'thread-existing'"
                    .to_owned(),
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.try_get::<String>("", "title").unwrap(), "Existing");
        assert_eq!(
            row.try_get::<String>("", "workspace_mode").unwrap(),
            "local",
            "existing Thread rows must be interpreted as local"
        );
        assert_eq!(
            row.try_get::<String>("", "workspace_path").unwrap(),
            "/workspace",
            "local rows must backfill their owning Project path"
        );
        let lease = db
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT schema_version, payload_json, payload_hash FROM studio_objects \
                 WHERE object_kind = 'worktreeLease'"
                    .to_owned(),
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(lease.try_get::<i64>("", "schema_version").unwrap(), 2);
        let payload_json = lease.try_get::<String>("", "payload_json").unwrap();
        let migrated: serde_json::Value = serde_json::from_str(&payload_json).unwrap();
        assert_eq!(migrated["ownerKind"], serde_json::json!("child"));
        assert_eq!(migrated["ownerThreadId"], serde_json::json!("child-1"));
        assert!(migrated.get("childId").is_none());
        assert_eq!(
            lease.try_get::<String>("", "payload_hash").unwrap(),
            pl_core::context::content_hash(payload_json.as_bytes())
        );

        // 重复启动幂等：第二次升级不再改写任何事实。
        upgrade_product_schema(&db).await.unwrap();
        assert_eq!(studio_schema_version(&db).await.unwrap(), 22);
        let again = db
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT payload_json FROM studio_objects WHERE object_kind = 'worktreeLease'"
                    .to_owned(),
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            again.try_get::<String>("", "payload_json").unwrap(),
            payload_json
        );
        let _ = db.close().await;
    }

    /// 升级后的 `threads` 表文本必须与全新创建的 schema 指纹一致。
    #[tokio::test]
    async fn upgraded_threads_table_matches_a_freshly_initialized_schema() {
        let directory = tempfile::tempdir().unwrap();
        let upgraded = open(&directory.path().join("upgraded.sqlite")).await;
        upgraded
            .execute_unprepared(VERSION_20_SCHEMA)
            .await
            .unwrap();
        upgrade_product_schema(&upgraded).await.unwrap();
        let upgraded_sql = table_sql(&upgraded, "threads").await;
        let _ = upgraded.close().await;

        let fresh = open(&directory.path().join("fresh.sqlite")).await;
        initialize_studio_schema(&fresh).await.unwrap();
        let fresh_sql = table_sql(&fresh, "threads").await;
        let _ = fresh.close().await;

        let normalize = |sql: String| sql.split_whitespace().collect::<Vec<_>>().join(" ");
        assert_eq!(
            normalize(upgraded_sql),
            normalize(fresh_sql),
            "ALTER TABLE migration text must match canonical creation, otherwise the schema fingerprint rejects existing data"
        );
    }

    /// v21 → v22：回填 `workspace_path`——`local` 取所属 Project 的 `path`，`worktree` 取该
    /// root Thread 的会话 lease `path`，子线程继承会话地址、缺失 lease 时回退 Project 的
    /// `path`；迁移幂等且不删除 Thread 行或 lease。
    #[tokio::test]
    async fn schema_21_to_22_backfills_thread_workspace_paths() {
        let directory = tempfile::tempdir().unwrap();
        let db = open(&directory.path().join("studio.sqlite")).await;
        db.execute_unprepared(VERSION_20_SCHEMA).await.unwrap();
        db.execute_unprepared(
            "ALTER TABLE threads ADD COLUMN workspace_mode TEXT NOT NULL DEFAULT 'local';
             PRAGMA user_version=21;",
        )
        .await
        .unwrap();
        db.execute_unprepared(
            "INSERT INTO projects (id, name, path, ssh_server_id, created_at, updated_at,
                last_opened_at, closed)
             VALUES ('project-1', 'Project', '/workspace', NULL, 1, 1, 1, 0);",
        )
        .await
        .unwrap();
        for (id, parent, root, mode) in [
            ("thread-local", "NULL", "thread-local", "local"),
            ("thread-worktree", "NULL", "thread-worktree", "worktree"),
            (
                "thread-child",
                "'thread-worktree'",
                "thread-worktree",
                "worktree",
            ),
            ("thread-orphan", "NULL", "thread-orphan", "worktree"),
        ] {
            db.execute_unprepared(&format!(
                "INSERT INTO threads (id, project_id, title, mode, root_thread_id, parent_thread_id,
                    role, agent_path, state_json, revision, runtime_revision, event_sequence,
                    metadata_json, usage_json, last_context_tokens, trace_sequence, created_at,
                    updated_at, archived, workspace_mode)
                 VALUES ('{id}', 'project-1', '{id}', 'mode.simple', '{root}', {parent},
                    'planner', '{id}', '{{\"kind\":\"idle\",\"error\":null}}', 0, 1, 0,
                    '{{}}', '{{}}', NULL, 0, 5, 7, 0, '{mode}');"
            ))
            .await
            .unwrap();
        }
        let lease = serde_json::json!({
            "revision": 2,
            "state": "active",
            "ownerKind": "session",
            "ownerThreadId": "thread-worktree",
            "rootThreadId": "thread-worktree",
            "projectId": "project-1",
            "sshAlias": null,
            "repositoryRoot": "/repo",
            "path": "/repo/.anywork/worktrees/thread-worktree/session",
            "branch": "pure-session-thread-worktree",
            "baseCommit": "base",
        })
        .to_string();
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO studio_objects (owner_kind, owner_id, object_kind, revision,
                schema_version, payload_json, payload_hash, updated_at)
             VALUES ('agent', 'thread-worktree', 'worktreeLease', 2, 2, ?, ?, 9)",
            [
                Value::String(Some(lease.clone())),
                Value::String(Some(pl_core::context::content_hash(lease.as_bytes()))),
            ],
        ))
        .await
        .unwrap();

        upgrade_product_schema(&db).await.unwrap();
        assert_eq!(studio_schema_version(&db).await.unwrap(), 22);
        assert_eq!(
            workspace_path(&db, "thread-local").await,
            "/workspace",
            "local rows take the canonical Project path"
        );
        assert_eq!(
            workspace_path(&db, "thread-worktree").await,
            "/repo/.anywork/worktrees/thread-worktree/session",
            "worktree rows take their root session lease path"
        );
        assert_eq!(
            workspace_path(&db, "thread-child").await,
            "/repo/.anywork/worktrees/thread-worktree/session",
            "child rows inherit the root session address"
        );
        assert_eq!(
            workspace_path(&db, "thread-orphan").await,
            "/workspace",
            "a missing session lease falls back to the Project path"
        );
        // 重复启动幂等：不再改写任何事实。
        upgrade_product_schema(&db).await.unwrap();
        assert_eq!(studio_schema_version(&db).await.unwrap(), 22);
        assert_eq!(
            workspace_path(&db, "thread-worktree").await,
            "/repo/.anywork/worktrees/thread-worktree/session"
        );
        let _ = db.close().await;
    }

    async fn workspace_path(db: &DatabaseConnection, id: &str) -> String {
        db.query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT workspace_path FROM threads WHERE id = ?",
            [Value::String(Some(id.to_owned()))],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "workspace_path")
        .unwrap()
    }

    async fn table_sql(db: &DatabaseConnection, name: &str) -> String {
        db.query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT sql FROM sqlite_schema WHERE name = ?",
            [Value::String(Some(name.to_owned()))],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "sql")
        .unwrap()
    }

    async fn version_21_database() -> DatabaseConnection {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        db.execute_unprepared(
            "CREATE TABLE projects(
                id TEXT PRIMARY KEY NOT NULL, name TEXT NOT NULL, path TEXT NOT NULL,
                ssh_alias TEXT, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
                last_opened_at INTEGER, closed INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE threads(
                id TEXT PRIMARY KEY NOT NULL, project_id TEXT NOT NULL, title TEXT NOT NULL,
                mode TEXT NOT NULL, root_thread_id TEXT NOT NULL, parent_thread_id TEXT,
                role TEXT NOT NULL, agent_path TEXT NOT NULL, state_json TEXT NOT NULL,
                revision INTEGER NOT NULL, event_sequence INTEGER NOT NULL,
                metadata_json TEXT NOT NULL, usage_json TEXT NOT NULL,
                trace_sequence INTEGER NOT NULL, created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL, archived INTEGER NOT NULL,
                workspace_mode TEXT NOT NULL DEFAULT 'local');
             CREATE TABLE studio_objects(
                owner_kind TEXT NOT NULL, owner_id TEXT NOT NULL, object_kind TEXT NOT NULL,
                revision INTEGER NOT NULL, schema_version INTEGER NOT NULL,
                payload_json TEXT NOT NULL, payload_hash TEXT NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY(owner_kind, owner_id, object_kind));
             PRAGMA user_version = 21;",
        )
        .await
        .unwrap();
        db
    }

    async fn insert_thread(db: &DatabaseConnection, thread: &str, project: &str, mode: &str) {
        db.execute_unprepared(&format!(
            "INSERT INTO threads(id, project_id, title, mode, root_thread_id, role, agent_path, \
             state_json, revision, event_sequence, metadata_json, usage_json, trace_sequence, \
             created_at, updated_at, archived, workspace_mode) \
             VALUES('{thread}','{project}','T','simple','{thread}','planner','{thread}', \
             '{{\"kind\":\"idle\"}}',0,0,'{{}}','{{}}',0,1,1,0,'{mode}')"
        ))
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn v21_backfill_rejects_a_missing_project_and_rolls_back() {
        let db = version_21_database().await;
        insert_thread(&db, "t1", "missing", "local").await;
        let error = upgrade_product_schema(&db).await.unwrap_err();
        assert!(error.to_string().contains("missing Project"), "{error}");
        assert_eq!(studio_schema_version(&db).await.unwrap(), 21);
        // The ALTER TABLE was rolled back; the schema must still be v21.
        assert!(
            db.query_all_raw(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT workspace_path FROM threads".to_owned(),
            ))
            .await
            .is_err()
        );
        db.close().await.unwrap();
    }

    #[tokio::test]
    async fn v21_backfill_rejects_an_empty_project_path() {
        let db = version_21_database().await;
        db.execute_unprepared(
            "INSERT INTO projects(id,name,path,created_at,updated_at,closed) \
             VALUES('p1','P','',1,1,0)",
        )
        .await
        .unwrap();
        insert_thread(&db, "t1", "p1", "local").await;
        let error = upgrade_product_schema(&db).await.unwrap_err();
        assert!(error.to_string().contains("empty path"), "{error}");
        assert_eq!(studio_schema_version(&db).await.unwrap(), 21);
        db.close().await.unwrap();
    }

    #[tokio::test]
    async fn v21_backfill_rejects_a_lease_whose_owner_does_not_match() {
        let db = version_21_database().await;
        insert_thread(&db, "w1", "p1", "worktree").await;
        let payload =
            "{\"ownerKind\":\"session\",\"ownerThreadId\":\"other\",\"path\":\"/tmp/wt\"}";
        let hash = pl_core::context::content_hash(payload.as_bytes());
        db.execute_unprepared(&format!(
            "INSERT INTO studio_objects(owner_kind,owner_id,object_kind,revision,schema_version,\
             payload_json,payload_hash,updated_at) \
             VALUES('agent','w1','worktreeLease',1,2,'{payload}','{hash}',1)"
        ))
        .await
        .unwrap();
        let error = upgrade_product_schema(&db).await.unwrap_err();
        assert!(error.to_string().contains("lease owner"), "{error}");
        assert_eq!(studio_schema_version(&db).await.unwrap(), 21);
        db.close().await.unwrap();
    }
}
