use crate::{ContentPart, ImageSource, Message, MessageContent, ModelContextItem};
use anyhow::Result;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseBackend,
    DatabaseConnection, DatabaseTransaction, EntityTrait, QueryFilter, Statement, TransactionTrait,
};

use crate::studio::entities;
use crate::studio::ids::new_id;
use crate::studio::mappers::message_to_row_parts;

pub(super) const STUDIO_DATABASE_SCHEMA_VERSION: i64 = 1;
const BASE_SCHEMA: &str = include_str!("../../migrations/0001_base.sql");

pub(super) async fn insert_message_with_tx(
    tx: &DatabaseTransaction,
    session_id: &str,
    message: &Message,
    now: i64,
) -> Result<()> {
    use entities::message as message_entity;
    let (role, content) = message_to_row_parts(message)?;
    let metadata_json = serde_json::to_string(&message.metadata)?;
    let message_id = new_id("message");
    message_entity::ActiveModel {
        id: Set(message_id.clone()),
        session_id: Set(session_id.to_string()),
        item_type: Set("message".to_string()),
        role: Set(role),
        content: Set(content),
        reasoning_content: Set(message.reasoning_content.clone()),
        metadata_json: Set(metadata_json),
        created_at: Set(now),
    }
    .insert(tx)
    .await?;
    bind_message_attachments_with_tx(tx, session_id, &message_id, message).await?;
    Ok(())
}

pub(super) async fn insert_context_item_with_tx(
    tx: &DatabaseTransaction,
    session_id: &str,
    item: &ModelContextItem,
    now: i64,
) -> Result<()> {
    match item {
        ModelContextItem::Message { message } => {
            insert_message_with_tx(tx, session_id, message, now).await
        }
        ModelContextItem::ToolResult { .. } | ModelContextItem::PinnedContext { .. } => {
            use entities::message as message_entity;
            message_entity::ActiveModel {
                id: Set(new_id("message")),
                session_id: Set(session_id.to_string()),
                item_type: Set("canonical".to_string()),
                role: Set(String::new()),
                content: Set(serde_json::to_string(item)?),
                reasoning_content: Set(None),
                metadata_json: Set("{}".to_string()),
                created_at: Set(now),
            }
            .insert(tx)
            .await?;
            Ok(())
        }
        ModelContextItem::Compaction { encrypted_content } => {
            use entities::message as message_entity;
            message_entity::ActiveModel {
                id: Set(new_id("message")),
                session_id: Set(session_id.to_string()),
                item_type: Set("compaction".to_string()),
                role: Set(String::new()),
                content: Set(encrypted_content.clone()),
                reasoning_content: Set(None),
                metadata_json: Set("{}".to_string()),
                created_at: Set(now),
            }
            .insert(tx)
            .await?;
            Ok(())
        }
    }
}

pub(super) async fn bind_message_attachments_with_tx(
    tx: &DatabaseTransaction,
    session_id: &str,
    message_id: &str,
    message: &Message,
) -> Result<()> {
    use entities::attachment;
    for attachment_id in attachment_ids(message) {
        if let Some(existing) = attachment::Entity::find_by_id(attachment_id.clone())
            .filter(attachment::Column::SessionId.eq(session_id.to_string()))
            .one(tx)
            .await?
        {
            let mut active: attachment::ActiveModel = existing.into();
            active.message_id = Set(Some(message_id.to_string()));
            active.update(tx).await?;
        }
    }
    Ok(())
}

pub(super) fn attachment_ids(message: &Message) -> Vec<String> {
    match &message.content {
        MessageContent::Text(_) => Vec::new(),
        MessageContent::MultiPart(parts) => parts
            .iter()
            .filter_map(|part| match part {
                ContentPart::Image {
                    source: ImageSource::Attachment { attachment_id },
                    ..
                } => Some(attachment_id.clone()),
                ContentPart::Text { .. } | ContentPart::Image { .. } => None,
            })
            .collect(),
    }
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

pub(super) async fn initialize_schema(db: &DatabaseConnection) -> Result<()> {
    let tx = db.begin().await?;
    for statement in split_sql(BASE_SCHEMA) {
        tx.execute(Statement::from_string(DatabaseBackend::Sqlite, statement))
            .await?;
    }
    tx.commit().await?;
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
