//! Studio product schema; configuration, projects and credential references survive session reset.
//! Session-format upgrades run only through the locked startup reset coordinator after backup.

use anyhow::{Context, Result, bail};
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
    tx.execute_unprepared(
        "UPDATE threads SET workspace_path = COALESCE( \
            (SELECT path FROM projects WHERE projects.id = threads.project_id), '') \
         WHERE workspace_mode = 'local';",
    )
    .await?;
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
                "SELECT payload_json FROM studio_objects \
                 WHERE owner_kind = 'agent' AND owner_id = ? AND object_kind = 'worktreeLease'",
                [Value::String(Some(root_thread_id))],
            ))
            .await?;
        let mut lease_path = None;
        if let Some(lease) = lease {
            let payload_json: String = lease.try_get("", "payload_json")?;
            let payload: serde_json::Value = serde_json::from_str(&payload_json)
                .with_context(|| format!("invalid worktree lease payload for {id}"))?;
            if payload.get("ownerKind").and_then(serde_json::Value::as_str) == Some("session") {
                lease_path = payload
                    .get("path")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned);
            }
        }
        let workspace_path = match lease_path {
            Some(path) => path,
            None => tx
                .query_one_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    "SELECT path FROM projects WHERE id = ?",
                    [Value::String(Some(project_id))],
                ))
                .await?
                .map(|project| project.try_get::<String>("", "path"))
                .transpose()?
                .unwrap_or_default(),
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
