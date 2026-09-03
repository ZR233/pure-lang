//! Studio 当前数据库 schema。
//!
//! v18 是轻量 Thread Mode workflow 持久化的破坏性边界。打开旧版本时由
//! `store::project` 删除整个数据库
//! family 并从这里重建，因此本模块不包含旧 Task 表或迁移逻辑。

use anyhow::Result;
use sea_orm::sea_query::{Index, IndexCreateStatement, IndexOrder};
use sea_orm::{ConnectionTrait, DatabaseConnection};

use crate::studio::entity;

pub(super) const STUDIO_DATABASE_SCHEMA_VERSION: i64 = 18;

pub(super) async fn initialize_studio_schema(db: &DatabaseConnection) -> Result<()> {
    create_thread_lifecycle_tables(db).await?;
    db.get_schema_builder()
        .register(entity::app_setting::Entity)
        .register(entity::ssh_server::Entity)
        .register(entity::project::Entity)
        .register(entity::attachment::Entity)
        .register(entity::thread_submission::Entity)
        .register(entity::thread_context_segment::Entity)
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

        CREATE TABLE IF NOT EXISTS thread_inputs (
            id TEXT PRIMARY KEY NOT NULL,
            thread_id TEXT NOT NULL,
            mail_id TEXT NOT NULL UNIQUE,
            turn_id TEXT NOT NULL,
            content TEXT NOT NULL,
            attachments_json TEXT NOT NULL CHECK (json_valid(attachments_json)),
            metadata_json TEXT NOT NULL,
            presentation TEXT NOT NULL,
            state_json TEXT NOT NULL CHECK (json_valid(state_json)),
            state_kind TEXT GENERATED ALWAYS AS (
                json_extract(state_json, '$.kind')
            ) STORED NOT NULL CHECK (state_kind IN ('pending', 'claimed', 'consumed')),
            queue_ordinal INTEGER NOT NULL,
            queued_at INTEGER NOT NULL,
            FOREIGN KEY (thread_id) REFERENCES threads(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS turns (
            id TEXT PRIMARY KEY NOT NULL,
            thread_id TEXT NOT NULL,
            ordinal INTEGER NOT NULL,
            revision INTEGER NOT NULL CHECK (revision >= 0),
            state_json TEXT NOT NULL CHECK (json_valid(state_json)),
            state_kind TEXT GENERATED ALWAYS AS (
                json_extract(state_json, '$.kind')
            ) STORED NOT NULL CHECK (
                state_kind IN (
                    'queued', 'running', 'completed', 'cancelled', 'failed',
                    'budgetLimited'
                )
            ),
            model_json TEXT,
            usage_json TEXT NOT NULL,
            metadata_json TEXT,
            updated_at INTEGER NOT NULL,
            FOREIGN KEY (thread_id) REFERENCES threads(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS items (
            id TEXT PRIMARY KEY NOT NULL,
            thread_id TEXT NOT NULL,
            turn_id TEXT NOT NULL,
            ordinal INTEGER NOT NULL,
            revision INTEGER NOT NULL CHECK (revision >= 0),
            state_json TEXT NOT NULL CHECK (json_valid(state_json)),
            state_kind TEXT GENERATED ALWAYS AS (
                json_extract(state_json, '$.kind')
            ) STORED NOT NULL CHECK (
                state_kind IN (
                    'text', 'thinking', 'tool', 'agent', 'turn', 'inference',
                    'plan', 'skill', 'file', 'contextCompaction'
                )
            ),
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            FOREIGN KEY (thread_id) REFERENCES threads(id) ON DELETE CASCADE,
            FOREIGN KEY (turn_id) REFERENCES turns(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS interactions (
            id TEXT PRIMARY KEY NOT NULL,
            thread_id TEXT NOT NULL,
            turn_id TEXT NOT NULL,
            item_id TEXT,
            tool_id TEXT,
            agent_path TEXT,
            revision INTEGER NOT NULL CHECK (revision >= 0),
            purpose_json TEXT NOT NULL CHECK (json_valid(purpose_json)),
            continuation_json TEXT NOT NULL CHECK (json_valid(continuation_json)),
            state_json TEXT NOT NULL CHECK (json_valid(state_json)),
            interaction_kind TEXT GENERATED ALWAYS AS (
                json_extract(state_json, '$.kind')
            ) STORED NOT NULL CHECK (
                interaction_kind IN ('userInput', 'toolApproval')
            ),
            state_kind TEXT GENERATED ALWAYS AS (
                json_extract(state_json, '$.data.state.kind')
            ) STORED NOT NULL CHECK (
                state_kind IN ('pending', 'resolved', 'cancelled', 'expired')
            ),
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            FOREIGN KEY (thread_id) REFERENCES threads(id) ON DELETE CASCADE
        )
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
    create_attachment_indexes(db).await?;
    create_project_indexes(db).await?;
    let indexes = [
        Index::create()
            .name("idx_interactions_thread_state_updated")
            .table(entity::interaction::Entity)
            .col(entity::interaction::Column::ThreadId)
            .col(entity::interaction::Column::StateKind)
            .col((entity::interaction::Column::UpdatedAt, IndexOrder::Desc))
            .to_owned(),
        Index::create()
            .name("idx_interactions_thread_turn")
            .table(entity::interaction::Entity)
            .col(entity::interaction::Column::ThreadId)
            .col(entity::interaction::Column::TurnId)
            .to_owned(),
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
        Index::create()
            .name("idx_thread_inputs_queue")
            .table(entity::thread_input::Entity)
            .col(entity::thread_input::Column::ThreadId)
            .col(entity::thread_input::Column::StateKind)
            .col(entity::thread_input::Column::QueueOrdinal)
            .unique()
            .to_owned(),
        Index::create()
            .name("idx_thread_submissions_ordinal")
            .table(entity::thread_submission::Entity)
            .col(entity::thread_submission::Column::ThreadId)
            .col(entity::thread_submission::Column::Ordinal)
            .unique()
            .to_owned(),
        Index::create()
            .name("idx_turns_thread_ordinal")
            .table(entity::turn::Entity)
            .col(entity::turn::Column::ThreadId)
            .col(entity::turn::Column::Ordinal)
            .unique()
            .to_owned(),
        Index::create()
            .name("idx_items_thread_ordinal")
            .table(entity::item::Entity)
            .col(entity::item::Column::ThreadId)
            .col(entity::item::Column::Ordinal)
            .unique()
            .to_owned(),
        Index::create()
            .name("idx_items_turn_ordinal")
            .table(entity::item::Entity)
            .col(entity::item::Column::TurnId)
            .col(entity::item::Column::Ordinal)
            .to_owned(),
        Index::create()
            .name("idx_items_thread_state_kind_ordinal")
            .table(entity::item::Entity)
            .col(entity::item::Column::ThreadId)
            .col(entity::item::Column::StateKind)
            .col((entity::item::Column::Ordinal, IndexOrder::Desc))
            .to_owned(),
        Index::create()
            .name("idx_thread_context_segments_thread_ordinal")
            .table(entity::thread_context_segment::Entity)
            .col(entity::thread_context_segment::Column::ThreadId)
            .col(entity::thread_context_segment::Column::Ordinal)
            .unique()
            .to_owned(),
        Index::create()
            .name("idx_thread_context_segments_thread_revision")
            .table(entity::thread_context_segment::Entity)
            .col(entity::thread_context_segment::Column::ThreadId)
            .col(entity::thread_context_segment::Column::Revision)
            .unique()
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

async fn create_attachment_indexes(db: &impl ConnectionTrait) -> Result<()> {
    let index = Index::create()
        .name("idx_attachments_thread_id")
        .table(entity::attachment::Entity)
        .col(entity::attachment::Column::ThreadId)
        .to_owned();
    db.execute(&index).await?;
    Ok(())
}

async fn execute_index(db: &DatabaseConnection, index: IndexCreateStatement) -> Result<()> {
    db.execute(&index).await?;
    Ok(())
}
