use anyhow::Result;
use pl_protocol::Message;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ConnectionTrait, DatabaseBackend, DatabaseConnection,
    DatabaseTransaction, EntityTrait, Statement,
};

use crate::studio::entities;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::mappers::message_to_row_parts;

const MIGRATIONS: &[&str] = &[
    include_str!("../../migrations/0001_init.sql"),
    include_str!("../../migrations/0002_subagent_events.sql"),
    include_str!("../../migrations/0003_session_runtime.sql"),
    include_str!("../../migrations/0004_trace_events.sql"),
    include_str!("../../migrations/0005_agent_events.sql"),
];

pub(super) async fn insert_message_with_tx(
    tx: &DatabaseTransaction,
    session_id: &str,
    message: &Message,
    now: i64,
) -> Result<()> {
    use entities::message as message_entity;
    let (role, content) = message_to_row_parts(message)?;
    let metadata_json = serde_json::to_string(&message.metadata)?;
    message_entity::ActiveModel {
        id: Set(new_id("message")),
        session_id: Set(session_id.to_string()),
        role: Set(role),
        content: Set(content),
        reasoning_content: Set(message.reasoning_content.clone()),
        metadata_json: Set(metadata_json),
        created_at: Set(now),
    }
    .insert(tx)
    .await?;
    Ok(())
}

pub(super) async fn touch_session_with_tx(
    tx: &DatabaseTransaction,
    session_id: &str,
    now: i64,
) -> Result<()> {
    use entities::session;
    if let Some(existing) = session::Entity::find_by_id(session_id.to_string())
        .one(tx)
        .await?
    {
        let mut active: session::ActiveModel = existing.into();
        active.updated_at = Set(now);
        active.update(tx).await?;
    }
    Ok(())
}

pub(super) async fn configure_sqlite(db: &DatabaseConnection) -> Result<()> {
    for pragma in [
        "PRAGMA journal_mode=WAL",
        "PRAGMA synchronous=NORMAL",
        "PRAGMA busy_timeout=5000",
        "PRAGMA foreign_keys=ON",
    ] {
        db.execute(Statement::from_string(
            DatabaseBackend::Sqlite,
            pragma.to_string(),
        ))
        .await?;
    }
    Ok(())
}

pub(super) async fn run_migrations(db: &DatabaseConnection) -> Result<()> {
    db.execute(Statement::from_string(
        DatabaseBackend::Sqlite,
        "CREATE TABLE IF NOT EXISTS studio_schema_migrations (
            version INTEGER PRIMARY KEY,
            applied_at INTEGER NOT NULL
        )"
        .to_string(),
    ))
    .await?;

    for (index, migration) in MIGRATIONS.iter().enumerate() {
        let version = index as i64 + 1;
        let applied = entities::app_setting::Entity::find_by_id(format!("migration:{version}"))
            .one(db)
            .await
            .unwrap_or(None)
            .is_some();
        if applied {
            continue;
        }

        for statement in split_sql(migration) {
            db.execute(Statement::from_string(DatabaseBackend::Sqlite, statement))
                .await?;
        }

        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO studio_schema_migrations (version, applied_at) VALUES (?, ?)",
            [version.into(), unix_seconds().into()],
        ))
        .await?;

        let _ = entities::app_setting::ActiveModel {
            key: Set(format!("migration:{version}")),
            value: Set("applied".to_string()),
            updated_at: Set(unix_seconds()),
        }
        .insert(db)
        .await;
    }
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

fn split_sql(sql: &str) -> Vec<String> {
    sql.split(';')
        .map(str::trim)
        .filter(|statement| !statement.is_empty())
        .map(|statement| format!("{statement};"))
        .collect()
}
