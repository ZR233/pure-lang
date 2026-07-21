use anyhow::Result;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
};

use crate::studio::entities;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::mappers::session_record;
use crate::studio::records::{SessionRecord, SessionVisibility};
use crate::studio::store::StudioStore;
use crate::studio::store_support::non_empty_title;
use crate::{InstructionSnapshot, StudioMode};

impl StudioStore {
    pub async fn create_session(
        &self,
        project_id: &str,
        title: &str,
        mode: StudioMode,
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

    pub async fn set_session_mode(&self, session_id: &str, mode: StudioMode) -> Result<()> {
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
}
