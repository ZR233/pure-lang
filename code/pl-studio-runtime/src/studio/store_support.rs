//! Studio 当前数据库 schema。
//!
//! v18 是轻量 Thread Mode workflow 持久化的破坏性边界。打开旧版本时由
//! `store::project` 删除整个数据库
//! family 并从这里重建，因此本模块不包含旧 Task 表或迁移逻辑。

use anyhow::Result;
use sea_orm::sea_query::{Index, IndexCreateStatement, IndexOrder};
use sea_orm::{ConnectionTrait, DatabaseConnection};

use crate::studio::entity;

pub(super) const STUDIO_DATABASE_SCHEMA_VERSION: i64 = 20;

pub(super) async fn initialize_studio_schema(db: &DatabaseConnection) -> Result<()> {
    create_thread_lifecycle_tables(db).await?;
    db.get_schema_builder()
        .register(entity::app_setting::Entity)
        .register(entity::ssh_server::Entity)
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
         ON projects(path) WHERE ssh_server_id IS NULL;
         CREATE UNIQUE INDEX idx_projects_remote_path
         ON projects(ssh_server_id, path) WHERE ssh_server_id IS NOT NULL;",
    )
    .await?;
    Ok(())
}

async fn execute_index(db: &DatabaseConnection, index: IndexCreateStatement) -> Result<()> {
    db.execute(&index).await?;
    Ok(())
}

/// One-time split: preserve all product facts and physical resources.
pub(super) async fn upgrade_session_storage(db: &DatabaseConnection) -> Result<()> {
    use sea_orm::{DatabaseBackend, Statement, TransactionTrait};
    let tx = db.begin().await?;
    let row = tx
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Sqlite,
            "PRAGMA user_version".to_owned(),
        ))
        .await?
        .ok_or_else(|| anyhow::anyhow!("missing Studio schema version"))?;
    match row.try_get::<i64>("", "user_version")? {
        20 => {}
        19 => {
            tx.execute_unprepared("DROP TABLE IF EXISTS attachments;
                DROP TABLE IF EXISTS thread_inputs;
                DROP TABLE IF EXISTS items;
                DROP TABLE IF EXISTS interactions;
                DROP TABLE IF EXISTS turns;
                DROP TABLE IF EXISTS thread_submissions;
                DROP TABLE IF EXISTS thread_context_segments;
                DELETE FROM studio_objects WHERE object_kind IN ('agentWorkingState','commitReceipt','modelPerformance');
                DELETE FROM threads;
                PRAGMA user_version=20;").await?;
        }
        version => anyhow::bail!("unsupported Studio schema {version}; product data preserved"),
    }
    tx.commit().await?;
    Ok(())
}
