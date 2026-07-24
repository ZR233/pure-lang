use anyhow::Result;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
};

use crate::studio::entities;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::mappers::session_record;
use crate::studio::records::{SessionKind, SessionRecord, SessionVisibility};
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
        let id = new_id("session");
        let model = session::ActiveModel {
            id: Set(id.clone()),
            project_id: Set(project_id.to_string()),
            title: Set(non_empty_title(title)),
            mode: Set(mode.label().to_string()),
            created_at: Set(now),
            updated_at: Set(now),
            archived: Set(0),
            visibility: Set(SessionVisibility::Active.as_str().to_string()),
            instruction_snapshot_json: Set(None),
            parent_session_id: Set(None),
            root_session_id: Set(id.clone()),
            session_kind: Set(SessionKind::Root.as_str().to_string()),
            owner_agent_id: Set(format!("studio:{id}")),
            owner_role: Set("planner".to_string()),
            agent_status: Set("idle".to_string()),
            agent_summary: Set(None),
            agent_error: Set(None),
            agent_updated_at: Set(Some(now)),
        }
        .insert(&self.db)
        .await?;
        Ok(session_record(model))
    }

    pub(in crate::studio) async fn create_agent_session(
        &self,
        spec: AgentSessionSpec,
    ) -> Result<SessionRecord> {
        use entities::session;
        if let Some(existing) = session::Entity::find_by_id(spec.id.clone())
            .one(&self.db)
            .await?
        {
            let existing = session_record(existing);
            anyhow::ensure!(
                existing.owner_agent_id == spec.owner_agent_id
                    && existing.parent_session_id.as_deref()
                        == Some(spec.parent_session_id.as_str()),
                "SessionId {} 已属于其他 agent 或父会话",
                spec.id
            );
            return Ok(existing);
        }
        let parent = session::Entity::find_by_id(spec.parent_session_id.clone())
            .one(&self.db)
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!("父 agent session 不存在: {}", spec.parent_session_id)
            })?;
        let now = unix_seconds();
        let model = session::ActiveModel {
            id: Set(spec.id),
            project_id: Set(parent.project_id),
            title: Set(non_empty_title(&spec.title)),
            mode: Set(parent.mode),
            created_at: Set(now),
            updated_at: Set(now),
            archived: Set(0),
            visibility: Set(SessionVisibility::Active.as_str().to_string()),
            instruction_snapshot_json: Set(parent.instruction_snapshot_json),
            parent_session_id: Set(Some(spec.parent_session_id)),
            root_session_id: Set(parent.root_session_id),
            session_kind: Set(SessionKind::Agent.as_str().to_string()),
            owner_agent_id: Set(spec.owner_agent_id),
            owner_role: Set(spec.owner_role),
            agent_status: Set("queued".to_string()),
            agent_summary: Set(None),
            agent_error: Set(None),
            agent_updated_at: Set(Some(now)),
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

    pub async fn list_all_sessions(&self, project_id: &str) -> Result<Vec<SessionRecord>> {
        use entities::session;
        let sessions = session::Entity::find()
            .filter(session::Column::ProjectId.eq(project_id.to_string()))
            .filter(session::Column::Mode.is_in(["simple", "task"]))
            .filter(session::Column::Archived.eq(0))
            .filter(session::Column::Visibility.eq(SessionVisibility::Active.as_str()))
            .order_by_desc(session::Column::UpdatedAt)
            .order_by_asc(session::Column::CreatedAt)
            .order_by_asc(session::Column::Id)
            .all(&self.db)
            .await?;
        Ok(sessions.into_iter().map(session_record).collect())
    }

    pub(in crate::studio) async fn list_active_agent_sessions(&self) -> Result<Vec<SessionRecord>> {
        use entities::session;
        let sessions = session::Entity::find()
            .filter(session::Column::SessionKind.eq(SessionKind::Agent.as_str()))
            .filter(session::Column::Archived.eq(0))
            .filter(session::Column::Visibility.eq(SessionVisibility::Active.as_str()))
            .order_by_asc(session::Column::CreatedAt)
            .order_by_asc(session::Column::Id)
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
        let targets = if archived.session_kind == SessionKind::Root {
            session::Entity::find()
                .filter(session::Column::RootSessionId.eq(session_id.to_string()))
                .all(&self.db)
                .await?
        } else {
            vec![existing]
        };
        for target in targets {
            let mut active: session::ActiveModel = target.into();
            active.archived = Set(1);
            active.visibility = Set(SessionVisibility::Archived.as_str().to_string());
            active.updated_at = Set(now);
            active.update(&self.db).await?;
        }
        Ok(Some(archived))
    }

    pub(in crate::studio) async fn update_agent_session_status(
        &self,
        session_id: &str,
        status: &str,
        summary: Option<String>,
        error: Option<String>,
        updated_at: i64,
    ) -> Result<()> {
        use entities::session;
        let Some(existing) = session::Entity::find_by_id(session_id.to_string())
            .one(&self.db)
            .await?
        else {
            return Ok(());
        };
        let mut active: session::ActiveModel = existing.into();
        active.agent_status = Set(status.to_string());
        active.agent_summary = Set(summary);
        active.agent_error = Set(error);
        active.agent_updated_at = Set(Some(updated_at));
        active.updated_at = Set(updated_at);
        active.update(&self.db).await?;
        Ok(())
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

#[derive(Debug, Clone)]
pub(in crate::studio) struct AgentSessionSpec {
    pub id: String,
    pub parent_session_id: String,
    pub owner_agent_id: String,
    pub owner_role: String,
    pub title: String,
}
