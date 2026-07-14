use anyhow::Result;
use pl_protocol::{Message, ModelContextItem};
use pl_trace::TraceEvent;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
    TransactionTrait,
};

use crate::studio::entities;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::mappers::{
    message_to_row_parts, row_to_context_item, row_to_message, session_record,
};
use crate::studio::records::{SessionRecord, SessionVisibility};
use crate::studio::store::StudioStore;
use crate::studio::store::skill;
use crate::studio::store_support::{
    insert_context_item_with_tx, insert_message_with_tx, non_empty_title, touch_session_with_tx,
};
use crate::{CompileMode, CoreSession, InstructionSnapshot};

impl StudioStore {
    pub async fn create_session(
        &self,
        project_id: &str,
        title: &str,
        mode: CompileMode,
    ) -> Result<SessionRecord> {
        use entities::session;
        let now = unix_seconds();
        let model = session::ActiveModel {
            id: Set(new_id("session")),
            project_id: Set(project_id.to_string()),
            title: Set(non_empty_title(title)),
            mode: Set(mode.label().to_string()),
            created_at: Set(now),
            updated_at: Set(now),
            archived: Set(0),
            visibility: Set(SessionVisibility::Active.as_str().to_string()),
            instruction_snapshot_json: Set(None),
            parent_session_id: Set(None),
        }
        .insert(&self.db)
        .await?;
        Ok(session_record(model))
    }

    pub async fn list_sessions(&self, project_id: &str) -> Result<Vec<SessionRecord>> {
        use entities::session;
        let sessions = session::Entity::find()
            .filter(session::Column::ProjectId.eq(project_id.to_string()))
            .filter(session::Column::Mode.is_in(["simple", "task"]))
            .filter(session::Column::Archived.eq(0))
            .filter(session::Column::Visibility.eq(SessionVisibility::Active.as_str()))
            .filter(session::Column::ParentSessionId.is_null())
            .order_by_desc(session::Column::UpdatedAt)
            .order_by_desc(session::Column::Id)
            .all(&self.db)
            .await?;
        Ok(sessions.into_iter().map(session_record).collect())
    }

    pub async fn list_project_session_ids(&self, project_id: &str) -> Result<Vec<String>> {
        use entities::session;
        let sessions = session::Entity::find()
            .filter(session::Column::ProjectId.eq(project_id.to_string()))
            .all(&self.db)
            .await?;
        Ok(sessions.into_iter().map(|session| session.id).collect())
    }

    pub async fn read_session(&self, session_id: &str) -> Result<Option<SessionRecord>> {
        use entities::session;
        Ok(session::Entity::find_by_id(session_id.to_string())
            .filter(session::Column::Mode.is_in(["simple", "task"]))
            .one(&self.db)
            .await?
            .map(session_record))
    }

    pub async fn load_core_session(&self, session_id: &str) -> Result<CoreSession> {
        Ok(CoreSession::from_items(
            self.load_context_items(session_id).await?,
        ))
    }

    pub async fn load_context_items(&self, session_id: &str) -> Result<Vec<ModelContextItem>> {
        use entities::message;
        let rows = message::Entity::find()
            .filter(message::Column::SessionId.eq(session_id.to_string()))
            .order_by_asc(message::Column::CreatedAt)
            .order_by_asc(message::Column::Id)
            .all(&self.db)
            .await?;
        rows.into_iter().map(row_to_context_item).collect()
    }

    pub async fn load_messages(&self, session_id: &str) -> Result<Vec<Message>> {
        use entities::message;
        let rows = message::Entity::find()
            .filter(message::Column::SessionId.eq(session_id.to_string()))
            .filter(message::Column::ItemType.eq("message"))
            .order_by_asc(message::Column::CreatedAt)
            .order_by_asc(message::Column::Id)
            .all(&self.db)
            .await?;
        rows.into_iter().map(row_to_message).collect()
    }

    pub async fn append_message(&self, session_id: &str, message: &Message) -> Result<()> {
        use entities::{message as message_entity, session};
        let now = unix_seconds();
        let (role, content) = message_to_row_parts(message)?;
        let metadata_json = serde_json::to_string(&message.metadata)?;
        message_entity::ActiveModel {
            id: Set(new_id("message")),
            session_id: Set(session_id.to_string()),
            item_type: Set("message".to_string()),
            role: Set(role),
            content: Set(content),
            reasoning_content: Set(message.reasoning_content.clone()),
            metadata_json: Set(metadata_json),
            created_at: Set(now),
        }
        .insert(&self.db)
        .await?;

        if let Some(existing) = session::Entity::find_by_id(session_id.to_string())
            .one(&self.db)
            .await?
        {
            let mut active: session::ActiveModel = existing.into();
            active.updated_at = Set(now);
            active.update(&self.db).await?;
        }
        Ok(())
    }

    pub async fn append_messages(&self, session_id: &str, messages: &[Message]) -> Result<()> {
        if messages.is_empty() {
            return Ok(());
        }
        let tx = self.db.begin().await?;
        let now = unix_seconds();
        for message in messages {
            insert_message_with_tx(&tx, session_id, message, now).await?;
        }
        touch_session_with_tx(&tx, session_id, now).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn replace_session_messages(
        &self,
        session_id: &str,
        messages: &[Message],
    ) -> Result<()> {
        let tx = self.db.begin().await?;
        replace_session_messages_with_tx(&tx, session_id, messages).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn append_context_items(
        &self,
        session_id: &str,
        items: &[ModelContextItem],
    ) -> Result<()> {
        if items.is_empty() {
            return Ok(());
        }
        let tx = self.db.begin().await?;
        let now = unix_seconds();
        for item in items {
            insert_context_item_with_tx(&tx, session_id, item, now).await?;
        }
        touch_session_with_tx(&tx, session_id, now).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn replace_session_context_items(
        &self,
        session_id: &str,
        items: &[ModelContextItem],
    ) -> Result<()> {
        let tx = self.db.begin().await?;
        replace_session_context_items_with_tx(&tx, session_id, items).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn rename_session(&self, session_id: &str, title: &str) -> Result<()> {
        use entities::session;
        if let Some(existing) = session::Entity::find_by_id(session_id.to_string())
            .one(&self.db)
            .await?
        {
            let now = unix_seconds();
            let mut active: session::ActiveModel = existing.into();
            active.title = Set(non_empty_title(title));
            active.updated_at = Set(now);
            active.update(&self.db).await?;
        }
        Ok(())
    }

    pub async fn archive_session(&self, session_id: &str) -> Result<Option<SessionRecord>> {
        use entities::session;
        let Some(existing) = session::Entity::find_by_id(session_id.to_string())
            .one(&self.db)
            .await?
        else {
            return Ok(None);
        };
        let archived = session_record(existing.clone());
        let now = unix_seconds();
        let mut active: session::ActiveModel = existing.into();
        active.archived = Set(1);
        active.visibility = Set(SessionVisibility::Archived.as_str().to_string());
        active.updated_at = Set(now);
        active.update(&self.db).await?;
        Ok(Some(archived))
    }

    pub async fn set_session_mode(&self, session_id: &str, mode: CompileMode) -> Result<()> {
        use entities::session;
        if let Some(existing) = session::Entity::find_by_id(session_id.to_string())
            .one(&self.db)
            .await?
        {
            let now = unix_seconds();
            let mut active: session::ActiveModel = existing.into();
            active.mode = Set(mode.label().to_string());
            active.updated_at = Set(now);
            active.update(&self.db).await?;
        }
        Ok(())
    }

    pub async fn save_instruction_snapshot(
        &self,
        session_id: &str,
        snapshot: &InstructionSnapshot,
    ) -> Result<Option<SessionRecord>> {
        use entities::session;
        let Some(existing) = session::Entity::find_by_id(session_id.to_string())
            .one(&self.db)
            .await?
        else {
            return Ok(None);
        };
        let now = unix_seconds();
        let mut active: session::ActiveModel = existing.into();
        active.instruction_snapshot_json = Set(Some(serde_json::to_string(snapshot)?));
        active.updated_at = Set(now);
        let model = active.update(&self.db).await?;
        Ok(Some(session_record(model)))
    }

    pub async fn append_turn_records(
        &self,
        session_id: &str,
        trace_events: &[TraceEvent],
        messages: &[Message],
    ) -> Result<()> {
        if trace_events.is_empty() && messages.is_empty() {
            return Ok(());
        }

        let tx = self.db.begin().await?;
        if !trace_events.is_empty() {
            // 旧 timeline_events 表不再作为运行期写入目标；turn 收尾只从内部
            // trace 中提取 skill 激活事件更新 skill 表（幂等）。
            skill::upsert_session_skill_events_with_tx(&tx, trace_events).await?;
        }
        if !messages.is_empty() {
            let now = unix_seconds();
            for message in messages {
                insert_message_with_tx(&tx, session_id, message, now).await?;
            }
            touch_session_with_tx(&tx, session_id, now).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn replace_turn_records(
        &self,
        session_id: &str,
        trace_events: &[TraceEvent],
        messages: &[Message],
    ) -> Result<()> {
        let tx = self.db.begin().await?;
        if !trace_events.is_empty() {
            // 同 append_turn_records：这里只更新 skill 表。
            skill::upsert_session_skill_events_with_tx(&tx, trace_events).await?;
        }
        replace_session_messages_with_tx(&tx, session_id, messages).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn append_turn_context_records(
        &self,
        session_id: &str,
        trace_events: &[TraceEvent],
        items: &[ModelContextItem],
    ) -> Result<()> {
        if trace_events.is_empty() && items.is_empty() {
            return Ok(());
        }
        let tx = self.db.begin().await?;
        if !trace_events.is_empty() {
            skill::upsert_session_skill_events_with_tx(&tx, trace_events).await?;
        }
        if !items.is_empty() {
            let now = unix_seconds();
            for item in items {
                insert_context_item_with_tx(&tx, session_id, item, now).await?;
            }
            touch_session_with_tx(&tx, session_id, now).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn replace_turn_context_records(
        &self,
        session_id: &str,
        trace_events: &[TraceEvent],
        items: &[ModelContextItem],
    ) -> Result<()> {
        let tx = self.db.begin().await?;
        if !trace_events.is_empty() {
            skill::upsert_session_skill_events_with_tx(&tx, trace_events).await?;
        }
        replace_session_context_items_with_tx(&tx, session_id, items).await?;
        tx.commit().await?;
        Ok(())
    }
}

async fn replace_session_messages_with_tx(
    tx: &sea_orm::DatabaseTransaction,
    session_id: &str,
    messages: &[Message],
) -> Result<()> {
    use entities::message;
    let now = unix_seconds();
    message::Entity::delete_many()
        .filter(message::Column::SessionId.eq(session_id.to_string()))
        .exec(tx)
        .await?;
    for message in messages {
        insert_message_with_tx(tx, session_id, message, now).await?;
    }
    touch_session_with_tx(tx, session_id, now).await?;
    Ok(())
}

async fn replace_session_context_items_with_tx(
    tx: &sea_orm::DatabaseTransaction,
    session_id: &str,
    items: &[ModelContextItem],
) -> Result<()> {
    use entities::message;
    let now = unix_seconds();
    message::Entity::delete_many()
        .filter(message::Column::SessionId.eq(session_id.to_string()))
        .exec(tx)
        .await?;
    for item in items {
        insert_context_item_with_tx(tx, session_id, item, now).await?;
    }
    touch_session_with_tx(tx, session_id, now).await?;
    Ok(())
}
