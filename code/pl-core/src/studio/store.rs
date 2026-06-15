use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use base64::Engine;
use image::GenericImageView;
use pl_protocol::{
    AgentEvent, AgentRuntimeDelta, InteractionKind, InteractionPayload, InteractionRequest,
    InteractionResolution, InteractionStatus, Message, PlanConfirmationResolution,
    PlanLifecycleEvent, PlanLifecycleState, RuntimeUsageSnapshot, SkillActivation,
    StudioAttachment, StudioEventEnvelope, StudioEventKind, StudioMessage, StudioPart,
    StudioTurnStatus, TimelineAttachment, TraceEvent, TraceEventKind,
};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectOptions, ConnectionTrait, Database,
    DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, QuerySelect, Statement,
    TransactionTrait,
};
use tokio::sync::Mutex;

use crate::runtime_usage::{
    ROOT_AGENT_ID, ROOT_AGENT_PATH, ROOT_AGENT_ROLE, aggregate_runtime_usage, cost_for_usage,
    merge_costs, token_usage_snapshot,
};
use crate::studio::entities;
use crate::studio::ids::{new_id, new_timeline_event_id, unix_seconds};
use crate::studio::mappers::{
    agent_runtime_snapshot_record, agent_snapshot_record, agent_timeline_event_record,
    attachment_record, costs_to_json, interaction_record, message_to_row_parts, project_record,
    row_to_message, session_handoff_record, session_record, session_runtime_record,
    session_skill_record, studio_event_envelope, studio_event_record, studio_message_record,
    studio_message_role_label, studio_message_status_label, studio_part_record,
    studio_part_status_label, studio_part_type_label, studio_text_channel_label,
    studio_turn_record, timeline_event_record, trace_event_kind_label,
};
use crate::studio::paths::{
    default_attachments_dir, prepare_database_switch, project_name, sqlite_url,
};
use crate::studio::records::{
    AgentSnapshotRecord, AgentTimelineEventRecord, AttachmentRecord, MaterializedAttachment,
    PlanImplementationHandoffStart, ProjectRecord, SessionHandoffKind, SessionHandoffStatus,
    SessionRecord, SessionRuntimeRecord, SessionSkillRecord, SessionVisibility,
    StudioMessageRecord, StudioPartRecord, StudioTurnRecord, TimelineEventRecord,
};
use crate::studio::store_support::{
    configure_sqlite, insert_message_with_tx, non_empty_title, run_migrations,
    touch_session_with_tx,
};
use crate::{CompileMode, CoreSession, InstructionSnapshot, TurnResult};

const TIMELINE_EVENT_INSERT_CHUNK_SIZE: usize = 100;

#[derive(Clone)]
pub struct StudioStore {
    db: DatabaseConnection,
    handoff_lock: Arc<Mutex<()>>,
}

impl StudioStore {
    pub async fn default_app() -> Result<Self> {
        let db_path = prepare_database_switch()?;
        Self::open(db_path).await
    }

    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let url = sqlite_url(path);
        Self::open_url(&url).await
    }

    pub async fn open_memory() -> Result<Self> {
        Self::open_url("sqlite::memory:").await
    }

    async fn open_url(url: &str) -> Result<Self> {
        let mut options = ConnectOptions::new(url.to_string());
        options
            .max_connections(1)
            .min_connections(1)
            .connect_timeout(Duration::from_secs(8))
            .acquire_timeout(Duration::from_secs(8))
            .sqlx_logging(false);
        let db = Database::connect(options).await?;
        configure_sqlite(&db).await?;
        run_migrations(&db).await?;
        Ok(Self {
            db,
            handoff_lock: Arc::new(Mutex::new(())),
        })
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

    pub async fn has_projects(&self) -> Result<bool> {
        use entities::project;
        Ok(project::Entity::find().one(&self.db).await?.is_some())
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

    pub async fn archive_project(&self, project_id: &str) -> Result<Option<ProjectRecord>> {
        use entities::{
            agent, agent_event, agent_runtime_event, agent_runtime_snapshot, interaction, message,
            project, session, session_handoff, session_runtime_snapshot, session_skill,
            timeline_event, tool_approval,
        };
        let Some(project) = project::Entity::find_by_id(project_id.to_string())
            .one(&self.db)
            .await?
        else {
            return Ok(None);
        };
        let tx = self.db.begin().await?;
        let session_ids = session::Entity::find()
            .filter(session::Column::ProjectId.eq(project_id.to_string()))
            .all(&tx)
            .await?
            .into_iter()
            .map(|session| session.id)
            .collect::<Vec<_>>();

        for session_id in &session_ids {
            let session_id = session_id.to_string();
            timeline_event::Entity::delete_many()
                .filter(timeline_event::Column::SessionId.eq(session_id.clone()))
                .exec(&tx)
                .await?;
            tx.execute(Statement::from_sql_and_values(
                sea_orm::DatabaseBackend::Sqlite,
                "DELETE FROM trace_events WHERE session_id = ?",
                [session_id.clone().into()],
            ))
            .await?;
            message::Entity::delete_many()
                .filter(message::Column::SessionId.eq(session_id.clone()))
                .exec(&tx)
                .await?;
            tool_approval::Entity::delete_many()
                .filter(tool_approval::Column::SessionId.eq(session_id.clone()))
                .exec(&tx)
                .await?;
            interaction::Entity::delete_many()
                .filter(interaction::Column::SessionId.eq(session_id.clone()))
                .exec(&tx)
                .await?;
            session_skill::Entity::delete_many()
                .filter(session_skill::Column::SessionId.eq(session_id.clone()))
                .exec(&tx)
                .await?;
            agent::Entity::delete_many()
                .filter(agent::Column::SessionId.eq(session_id.clone()))
                .exec(&tx)
                .await?;
            agent_event::Entity::delete_many()
                .filter(agent_event::Column::SessionId.eq(session_id.clone()))
                .exec(&tx)
                .await?;
            agent_runtime_event::Entity::delete_many()
                .filter(agent_runtime_event::Column::SessionId.eq(session_id.clone()))
                .exec(&tx)
                .await?;
            agent_runtime_snapshot::Entity::delete_many()
                .filter(agent_runtime_snapshot::Column::SessionId.eq(session_id.clone()))
                .exec(&tx)
                .await?;
            session_runtime_snapshot::Entity::delete_many()
                .filter(session_runtime_snapshot::Column::SessionId.eq(session_id))
                .exec(&tx)
                .await?;
        }
        session_handoff::Entity::delete_many()
            .filter(session_handoff::Column::ProjectId.eq(project_id.to_string()))
            .exec(&tx)
            .await?;
        for legacy_table in ["agent_messages", "agent_turns"] {
            tx.execute(Statement::from_sql_and_values(
                sea_orm::DatabaseBackend::Sqlite,
                format!(
                    "DELETE FROM {legacy_table} WHERE session_id IN (
                        SELECT id FROM sessions WHERE project_id = ?
                    )"
                ),
                [project_id.to_string().into()],
            ))
            .await?;
        }
        session::Entity::delete_many()
            .filter(session::Column::ProjectId.eq(project_id.to_string()))
            .exec(&tx)
            .await?;

        let mut active: project::ActiveModel = project.into();
        active.updated_at = Set(unix_seconds());
        active.closed = Set(1);
        let model = active.update(&tx).await?;
        tx.commit().await?;
        Ok(Some(project_record(model)))
    }

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
        }
        .insert(&self.db)
        .await?;
        Ok(session_record(model))
    }

    pub async fn list_sessions(&self, project_id: &str) -> Result<Vec<SessionRecord>> {
        use entities::session;
        let sessions = session::Entity::find()
            .filter(session::Column::ProjectId.eq(project_id.to_string()))
            .filter(session::Column::Archived.eq(0))
            .filter(session::Column::Visibility.eq(SessionVisibility::Active.as_str()))
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

    pub async fn read_project(&self, project_id: &str) -> Result<Option<ProjectRecord>> {
        use entities::project;
        Ok(project::Entity::find_by_id(project_id.to_string())
            .one(&self.db)
            .await?
            .map(project_record))
    }

    pub async fn read_session(&self, session_id: &str) -> Result<Option<SessionRecord>> {
        use entities::session;
        Ok(session::Entity::find_by_id(session_id.to_string())
            .one(&self.db)
            .await?
            .map(session_record))
    }

    pub async fn load_core_session(&self, session_id: &str) -> Result<CoreSession> {
        Ok(CoreSession::from_messages(
            self.load_messages(session_id).await?,
        ))
    }

    pub async fn load_messages(&self, session_id: &str) -> Result<Vec<Message>> {
        use entities::message;
        let rows = message::Entity::find()
            .filter(message::Column::SessionId.eq(session_id.to_string()))
            .order_by_asc(message::Column::CreatedAt)
            .order_by_asc(message::Column::Id)
            .all(&self.db)
            .await?;
        rows.into_iter().map(row_to_message).collect()
    }

    pub async fn create_image_attachment(
        &self,
        session_id: &str,
        data_url: &str,
        filename: Option<String>,
    ) -> Result<AttachmentRecord> {
        let (media_type, bytes) = decode_image_data_url(data_url)?;
        let decoded_image =
            image::load_from_memory(&bytes).with_context(|| "failed to decode image attachment")?;
        let normalized = normalize_image_attachment(media_type, bytes, decoded_image)?;
        let attachment_id = new_id("attachment");
        let extension = extension_for_media_type(normalized.media_type)?;
        let dir = default_attachments_dir()?.join(session_id);
        tokio::fs::create_dir_all(&dir).await?;
        let storage_path = dir.join(format!("{attachment_id}.{extension}"));
        tokio::fs::write(&storage_path, &normalized.bytes).await?;

        use entities::attachment;
        let now = unix_seconds();
        let row = attachment::ActiveModel {
            id: Set(attachment_id),
            session_id: Set(session_id.to_string()),
            message_id: Set(None),
            media_type: Set(normalized.media_type.to_string()),
            filename: Set(filename.filter(|name| !name.trim().is_empty())),
            storage_path: Set(storage_path.to_string_lossy().to_string()),
            byte_size: Set(normalized.bytes.len() as i64),
            width: Set(Some(normalized.dimensions.0 as i64)),
            height: Set(Some(normalized.dimensions.1 as i64)),
            created_at: Set(now),
        }
        .insert(&self.db)
        .await?;
        Ok(attachment_record(row))
    }

    pub async fn list_session_attachments(
        &self,
        session_id: &str,
    ) -> Result<Vec<AttachmentRecord>> {
        use entities::attachment;
        let rows = attachment::Entity::find()
            .filter(attachment::Column::SessionId.eq(session_id.to_string()))
            .order_by_asc(attachment::Column::CreatedAt)
            .order_by_asc(attachment::Column::Id)
            .all(&self.db)
            .await?;
        Ok(rows.into_iter().map(attachment_record).collect())
    }

    pub async fn load_attachments(
        &self,
        session_id: &str,
        attachment_ids: &[String],
    ) -> Result<Vec<AttachmentRecord>> {
        if attachment_ids.is_empty() {
            return Ok(Vec::new());
        }
        use entities::attachment;
        let rows = attachment::Entity::find()
            .filter(attachment::Column::SessionId.eq(session_id.to_string()))
            .filter(attachment::Column::Id.is_in(attachment_ids.iter().cloned()))
            .all(&self.db)
            .await?;
        Ok(rows.into_iter().map(attachment_record).collect())
    }

    pub async fn materialize_session_attachments(
        &self,
        session_id: &str,
    ) -> Result<Vec<MaterializedAttachment>> {
        let records = self.list_session_attachments(session_id).await?;
        materialize_attachments(records).await
    }

    pub async fn materialize_attachments(
        &self,
        session_id: &str,
        attachment_ids: &[String],
    ) -> Result<Vec<MaterializedAttachment>> {
        let records = self.load_attachments(session_id, attachment_ids).await?;
        materialize_attachments(records).await
    }

    pub async fn append_message(&self, session_id: &str, message: &Message) -> Result<()> {
        use entities::{message as message_entity, session};
        let now = unix_seconds();
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

    pub async fn start_plan_implementation_handoff(
        &self,
        interaction_id: &str,
        resolution: InteractionResolution,
    ) -> Result<PlanImplementationHandoffStart> {
        let _guard = self.handoff_lock.lock().await;
        let tx = self.db.begin().await?;
        let result =
            start_plan_implementation_handoff_with_tx(&tx, interaction_id, resolution).await;
        match result {
            Ok(start) => {
                tx.commit().await?;
                Ok(start)
            }
            Err(error) => {
                tx.rollback().await?;
                Err(error)
            }
        }
    }

    pub async fn set_plan_implementation_handoff_status(
        &self,
        origin_session_id: &str,
        plan_id: &str,
        status: SessionHandoffStatus,
    ) -> Result<()> {
        use entities::session_handoff;
        let Some(existing) = session_handoff::Entity::find()
            .filter(session_handoff::Column::OriginSessionId.eq(origin_session_id.to_string()))
            .filter(session_handoff::Column::PlanId.eq(plan_id.to_string()))
            .filter(
                session_handoff::Column::Kind.eq(SessionHandoffKind::PlanImplementation.as_str()),
            )
            .one(&self.db)
            .await?
        else {
            return Ok(());
        };
        let mut active: session_handoff::ActiveModel = existing.into();
        active.status = Set(status.as_str().to_string());
        active.updated_at = Set(unix_seconds());
        active.update(&self.db).await?;
        Ok(())
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

    pub async fn upsert_interaction(&self, interaction: &InteractionRequest) -> Result<()> {
        use entities::interaction;
        let payload_json = serde_json::to_string(&interaction.payload)?;
        let resolution_json = interaction
            .resolution
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        if let Some(existing) = interaction::Entity::find_by_id(interaction.interaction_id.clone())
            .one(&self.db)
            .await?
        {
            let mut active: interaction::ActiveModel = existing.into();
            active.session_id = Set(interaction.scope.session_id.clone());
            active.turn_id = Set(interaction.scope.turn_id.clone());
            active.item_id = Set(interaction.scope.item_id.clone());
            active.tool_id = Set(interaction.scope.tool_id.clone());
            active.agent_path = Set(interaction.scope.agent_path.clone());
            active.kind = Set(interaction.kind.as_str().to_string());
            active.status = Set(interaction.status.as_str().to_string());
            active.payload_json = Set(payload_json);
            active.resolution_json = Set(resolution_json);
            active.updated_at = Set(interaction.updated_at);
            active.resolved_at = Set(interaction.resolved_at);
            active.update(&self.db).await?;
        } else {
            interaction::ActiveModel {
                id: Set(interaction.interaction_id.clone()),
                session_id: Set(interaction.scope.session_id.clone()),
                turn_id: Set(interaction.scope.turn_id.clone()),
                item_id: Set(interaction.scope.item_id.clone()),
                tool_id: Set(interaction.scope.tool_id.clone()),
                agent_path: Set(interaction.scope.agent_path.clone()),
                kind: Set(interaction.kind.as_str().to_string()),
                status: Set(interaction.status.as_str().to_string()),
                payload_json: Set(payload_json),
                resolution_json: Set(resolution_json),
                created_at: Set(interaction.created_at),
                updated_at: Set(interaction.updated_at),
                resolved_at: Set(interaction.resolved_at),
            }
            .insert(&self.db)
            .await?;
        }
        Ok(())
    }

    pub async fn read_interaction(
        &self,
        interaction_id: &str,
    ) -> Result<Option<InteractionRequest>> {
        use entities::interaction;
        interaction::Entity::find_by_id(interaction_id.to_string())
            .one(&self.db)
            .await?
            .map(interaction_record)
            .transpose()
    }

    pub async fn list_pending_interactions(
        &self,
        session_id: &str,
    ) -> Result<Vec<InteractionRequest>> {
        use entities::interaction;
        interaction::Entity::find()
            .filter(interaction::Column::SessionId.eq(session_id.to_string()))
            .filter(interaction::Column::Status.eq("pending"))
            .order_by_desc(interaction::Column::UpdatedAt)
            .order_by_desc(interaction::Column::Id)
            .all(&self.db)
            .await?
            .into_iter()
            .map(interaction_record)
            .collect()
    }

    pub async fn upsert_agent_snapshot(&self, record: AgentSnapshotRecord) -> Result<()> {
        use entities::agent;
        if let Some(existing) = agent::Entity::find_by_id(record.id.clone())
            .one(&self.db)
            .await?
        {
            let mut active: agent::ActiveModel = existing.into();
            active.session_id = Set(record.session_id);
            active.path = Set(record.path);
            active.parent_path = Set(record.parent_path);
            active.role = Set(record.role);
            active.task = Set(record.task);
            active.status = Set(record.status.as_str().to_string());
            active.summary = Set(record.summary);
            active.error = Set(record.error);
            active.reason = Set(record.reason);
            active.budget_limit_kind = Set(record
                .budget_limit_kind
                .map(|kind| kind.as_str().to_string()));
            active.budget_usage_json = Set(record
                .budget_usage
                .and_then(|usage| serde_json::to_string(&usage).ok()));
            active.depth = Set(record.depth);
            active.updated_at = Set(record.updated_at);
            active.update(&self.db).await?;
        } else {
            agent::ActiveModel {
                id: Set(record.id),
                session_id: Set(record.session_id),
                path: Set(record.path),
                parent_path: Set(record.parent_path),
                role: Set(record.role),
                task: Set(record.task),
                status: Set(record.status.as_str().to_string()),
                summary: Set(record.summary),
                error: Set(record.error),
                reason: Set(record.reason),
                budget_limit_kind: Set(record
                    .budget_limit_kind
                    .map(|kind| kind.as_str().to_string())),
                budget_usage_json: Set(record
                    .budget_usage
                    .and_then(|usage| serde_json::to_string(&usage).ok())),
                depth: Set(record.depth),
                updated_at: Set(record.updated_at),
            }
            .insert(&self.db)
            .await?;
        }
        Ok(())
    }

    pub async fn list_agents(&self, session_id: &str) -> Result<Vec<AgentSnapshotRecord>> {
        use entities::agent;
        let rows = agent::Entity::find()
            .filter(agent::Column::SessionId.eq(session_id.to_string()))
            .order_by_asc(agent::Column::Path)
            .all(&self.db)
            .await?;
        let runtime_by_agent = self.agent_runtime_usage_by_agent(session_id).await?;
        Ok(rows
            .into_iter()
            .map(agent_snapshot_record)
            .map(|mut record| {
                record.runtime_usage = runtime_by_agent.get(&record.id).cloned();
                record
            })
            .collect())
    }

    pub async fn agent_runtime_usage_by_agent(
        &self,
        session_id: &str,
    ) -> Result<HashMap<String, RuntimeUsageSnapshot>> {
        use entities::agent_runtime_snapshot;
        let rows = agent_runtime_snapshot::Entity::find()
            .filter(agent_runtime_snapshot::Column::SessionId.eq(session_id.to_string()))
            .all(&self.db)
            .await?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let agent_id = row.agent_id.clone();
                (agent_id, agent_runtime_snapshot_record(row))
            })
            .collect())
    }

    pub async fn record_agent_event(&self, record: AgentTimelineEventRecord) -> Result<()> {
        use entities::agent_event;
        agent_event::ActiveModel {
            id: Set(record.event_id),
            session_id: Set(record.session_id),
            sequence: Set(record.sequence),
            kind: Set(record.kind),
            agent_id: Set(record.agent_id),
            path: Set(record.path),
            parent_path: Set(record.parent_path),
            payload_json: Set(record.payload_json),
            created_at: Set(record.created_at),
        }
        .insert(&self.db)
        .await?;
        Ok(())
    }

    pub async fn list_agent_events(
        &self,
        session_id: &str,
    ) -> Result<Vec<AgentTimelineEventRecord>> {
        use entities::agent_event;
        let rows = agent_event::Entity::find()
            .filter(agent_event::Column::SessionId.eq(session_id.to_string()))
            .order_by_asc(agent_event::Column::Sequence)
            .all(&self.db)
            .await?;
        Ok(rows.into_iter().map(agent_timeline_event_record).collect())
    }

    pub async fn read_agent_event(
        &self,
        event_id: &str,
    ) -> Result<Option<AgentTimelineEventRecord>> {
        use entities::agent_event;
        Ok(agent_event::Entity::find_by_id(event_id.to_string())
            .one(&self.db)
            .await?
            .map(agent_timeline_event_record))
    }

    pub async fn next_agent_event_sequence(&self, session_id: &str) -> Result<i64> {
        use entities::agent_event;
        let max_seq = agent_event::Entity::find()
            .filter(agent_event::Column::SessionId.eq(session_id.to_string()))
            .order_by_desc(agent_event::Column::Sequence)
            .one(&self.db)
            .await?
            .map(|row| row.sequence);
        Ok(max_seq.map(|sequence| sequence + 1).unwrap_or(0))
    }

    pub async fn next_studio_event_sequence(&self, session_id: &str) -> Result<i64> {
        use entities::studio_event;
        let max_seq = studio_event::Entity::find()
            .filter(studio_event::Column::SessionId.eq(session_id.to_string()))
            .order_by_desc(studio_event::Column::Sequence)
            .one(&self.db)
            .await?
            .map(|row| row.sequence);
        Ok(max_seq.map(|sequence| sequence + 1).unwrap_or(0))
    }

    pub async fn append_studio_event(
        &self,
        mut envelope: StudioEventEnvelope,
    ) -> Result<StudioEventEnvelope> {
        if matches!(envelope.kind, StudioEventKind::MessagePartDelta { .. }) {
            bail!("messagePartDelta is live-only and must not be persisted");
        }
        let tx = self.db.begin().await?;
        if let Some(session_id) = envelope.session_id.as_deref() {
            let next_sequence = next_studio_event_sequence_with_tx(&tx, session_id).await?;
            envelope.sequence = next_sequence as u64;
        }
        envelope = canonicalize_studio_event_with_connection(&tx, envelope).await?;
        apply_studio_event_projection_with_tx(&tx, &envelope).await?;
        insert_studio_event_with_tx(&tx, &envelope).await?;
        tx.commit().await?;
        Ok(envelope)
    }

    pub async fn load_studio_events(
        &self,
        session_id: &str,
        after_sequence: Option<i64>,
        limit: Option<i64>,
    ) -> Result<Vec<StudioEventEnvelope>> {
        use entities::studio_event;
        let mut query = studio_event::Entity::find()
            .filter(studio_event::Column::SessionId.eq(session_id.to_string()))
            .order_by_asc(studio_event::Column::Sequence)
            .order_by_asc(studio_event::Column::Id);
        if let Some(after_sequence) = after_sequence {
            query = query.filter(studio_event::Column::Sequence.gt(after_sequence));
        }
        if let Some(limit) = limit.and_then(|value| u64::try_from(value).ok()) {
            query = query.limit(limit);
        }
        let mut envelopes = Vec::new();
        for row in query.all(&self.db).await? {
            envelopes.push(studio_event_envelope(studio_event_record(row))?);
        }
        Ok(envelopes)
    }

    pub async fn load_studio_messages(&self, session_id: &str) -> Result<Vec<StudioMessageRecord>> {
        use entities::studio_message;
        let rows = studio_message::Entity::find()
            .filter(studio_message::Column::SessionId.eq(session_id.to_string()))
            .order_by_asc(studio_message::Column::CreatedAt)
            .order_by_asc(studio_message::Column::Sequence)
            .order_by_asc(studio_message::Column::Id)
            .all(&self.db)
            .await?;
        rows.into_iter().map(studio_message_record).collect()
    }

    pub async fn load_message_parts(&self, session_id: &str) -> Result<Vec<StudioPartRecord>> {
        use entities::message_part;
        let rows = message_part::Entity::find()
            .filter(message_part::Column::SessionId.eq(session_id.to_string()))
            .order_by_asc(message_part::Column::PartOrder)
            .order_by_asc(message_part::Column::Sequence)
            .order_by_asc(message_part::Column::Id)
            .all(&self.db)
            .await?;
        rows.into_iter().map(studio_part_record).collect()
    }

    pub async fn create_turn(
        &self,
        session_id: &str,
        turn_id: &str,
        status: StudioTurnStatus,
        now: i64,
    ) -> Result<StudioTurnRecord> {
        use entities::turn;
        let existing = turn::Entity::find_by_id(turn_id.to_string())
            .one(&self.db)
            .await?;
        if let Some(existing) = existing {
            return Ok(studio_turn_record(existing));
        }
        let row = turn::ActiveModel {
            id: Set(turn_id.to_string()),
            session_id: Set(session_id.to_string()),
            status: Set(status.as_str().to_string()),
            reason: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
            completed_at: Set(None),
        }
        .insert(&self.db)
        .await?;
        Ok(studio_turn_record(row))
    }

    pub async fn set_turn_status(
        &self,
        turn_id: &str,
        status: StudioTurnStatus,
        reason: Option<String>,
        now: i64,
    ) -> Result<Option<StudioTurnRecord>> {
        use entities::turn;
        let Some(existing) = turn::Entity::find_by_id(turn_id.to_string())
            .one(&self.db)
            .await?
        else {
            return Ok(None);
        };
        let mut active: turn::ActiveModel = existing.into();
        active.status = Set(status.as_str().to_string());
        active.reason = Set(reason);
        active.updated_at = Set(now);
        active.completed_at = Set(if is_terminal_turn_status(status) {
            Some(now)
        } else {
            None
        });
        let row = active.update(&self.db).await?;
        Ok(Some(studio_turn_record(row)))
    }

    pub async fn cancel_unfinished_turns(&self, reason: &str) -> Result<Vec<StudioTurnRecord>> {
        use entities::turn;
        let now = unix_seconds();
        let rows = turn::Entity::find()
            .filter(turn::Column::Status.is_not_in([
                StudioTurnStatus::Completed.as_str(),
                StudioTurnStatus::Failed.as_str(),
                StudioTurnStatus::Cancelled.as_str(),
            ]))
            .all(&self.db)
            .await?;
        let mut cancelled = Vec::new();
        for row in rows {
            let mut active: turn::ActiveModel = row.into();
            active.status = Set(StudioTurnStatus::Cancelled.as_str().to_string());
            active.reason = Set(Some(reason.to_string()));
            active.updated_at = Set(now);
            active.completed_at = Set(Some(now));
            cancelled.push(studio_turn_record(active.update(&self.db).await?));
        }
        Ok(cancelled)
    }

    pub async fn append_timeline_events(&self, events: &[TraceEvent]) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }
        use entities::timeline_event;
        for chunk in events.chunks(TIMELINE_EVENT_INSERT_CHUNK_SIZE) {
            let models = timeline_event_models(chunk);
            timeline_event::Entity::insert_many(models)
                .on_empty_do_nothing()
                .on_conflict(timeline_event_sequence_conflict())
                .exec(&self.db)
                .await?;
        }
        Ok(())
    }

    pub async fn append_turn_records(
        &self,
        session_id: &str,
        timeline_events: &[TraceEvent],
        messages: &[Message],
    ) -> Result<()> {
        if timeline_events.is_empty() && messages.is_empty() {
            return Ok(());
        }

        let tx = self.db.begin().await?;
        if !timeline_events.is_empty() {
            // timeline 表由实时 emit 单源写入（apply_studio_event_projection 用 DB 全局
            // envelope.sequence），turn 结束不再重复写 timeline 表（消除双写与 sequence 双空间）。
            // 这里仅提取 skill 激活事件更新 skill 表（幂等）。
            upsert_session_skill_events_with_tx(&tx, timeline_events).await?;
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
        timeline_events: &[TraceEvent],
        messages: &[Message],
    ) -> Result<()> {
        let tx = self.db.begin().await?;
        if !timeline_events.is_empty() {
            // 同 append_turn_records：timeline 表由实时 emit 单源写入，这里只更新 skill 表。
            upsert_session_skill_events_with_tx(&tx, timeline_events).await?;
        }
        replace_session_messages_with_tx(&tx, session_id, messages).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn upsert_session_skill(
        &self,
        session_id: &str,
        activation: &SkillActivation,
    ) -> Result<()> {
        let tx = self.db.begin().await?;
        upsert_session_skill_with_tx(&tx, session_id, activation).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn list_session_skills(&self, session_id: &str) -> Result<Vec<SessionSkillRecord>> {
        use entities::session_skill;
        let rows = session_skill::Entity::find()
            .filter(session_skill::Column::SessionId.eq(session_id.to_string()))
            .order_by_desc(session_skill::Column::UpdatedAt)
            .order_by_asc(session_skill::Column::SkillNameKey)
            .all(&self.db)
            .await?;
        Ok(rows.into_iter().map(session_skill_record).collect())
    }

    pub async fn list_session_skill_names(&self, session_id: &str) -> Result<Vec<String>> {
        Ok(self
            .list_session_skills(session_id)
            .await?
            .into_iter()
            .map(|skill| skill.skill_name)
            .collect())
    }

    pub async fn load_timeline_events(
        &self,
        session_id: &str,
        after_sequence: Option<i64>,
        limit: Option<i64>,
    ) -> Result<Vec<TimelineEventRecord>> {
        use entities::timeline_event;
        let mut query = timeline_event::Entity::find()
            .filter(timeline_event::Column::SessionId.eq(session_id.to_string()));
        if let Some(after) = after_sequence {
            query = query.filter(timeline_event::Column::Sequence.gt(after));
        }
        query = query.order_by_asc(timeline_event::Column::Sequence);
        if let Some(limit) = limit {
            query = query.limit(limit as u64);
        }
        let rows = query.all(&self.db).await?;
        Ok(rows.into_iter().map(timeline_event_record).collect())
    }

    pub async fn next_timeline_sequence(&self, session_id: &str) -> Result<u64> {
        use entities::timeline_event;
        let max_seq: Option<i64> = timeline_event::Entity::find()
            .filter(timeline_event::Column::SessionId.eq(session_id.to_string()))
            .order_by_desc(timeline_event::Column::Sequence)
            .one(&self.db)
            .await?
            .map(|row| row.sequence);
        Ok(max_seq.map(|s| (s + 1) as u64).unwrap_or(0))
    }

    pub async fn record_agent_runtime_delta(
        &self,
        session_id: &str,
        delta: &AgentRuntimeDelta,
    ) -> Result<bool> {
        let tx = self.db.begin().await?;
        let updated = record_agent_runtime_delta_with_tx(&tx, session_id, delta).await?;
        tx.commit().await?;
        Ok(updated)
    }

    pub async fn rebuild_session_runtime_from_agent_snapshots(
        &self,
        session_id: &str,
    ) -> Result<()> {
        let tx = self.db.begin().await?;
        rebuild_session_runtime_from_agent_snapshots_with_tx(&tx, session_id).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn load_session_runtime(
        &self,
        session_id: &str,
    ) -> Result<Option<SessionRuntimeRecord>> {
        use entities::session_runtime_snapshot;
        Ok(
            session_runtime_snapshot::Entity::find_by_id(session_id.to_string())
                .one(&self.db)
                .await?
                .map(session_runtime_record),
        )
    }

    pub async fn upsert_session_runtime(
        &self,
        session_id: &str,
        result: &TurnResult,
        model: Option<&pl_model::ModelInfo>,
    ) -> Result<()> {
        use entities::agent_runtime_snapshot;

        let has_agent_runtime = agent_runtime_snapshot::Entity::find()
            .filter(agent_runtime_snapshot::Column::SessionId.eq(session_id.to_string()))
            .one(&self.db)
            .await?
            .is_some();
        if has_agent_runtime {
            return self
                .rebuild_session_runtime_from_agent_snapshots(session_id)
                .await;
        }

        let usage = token_usage_snapshot(&result.usage);
        if usage.total_tokens == 0 {
            return Ok(());
        }
        let model_name = if result.model.is_empty() {
            model
                .map(|model| model.slug.clone())
                .unwrap_or_else(|| "unknown".to_string())
        } else {
            result.model.clone()
        };
        let (estimated_costs, has_unpriced_usage) = cost_for_usage(&usage, model);
        let delta = AgentRuntimeDelta {
            inference_id: new_id("legacy-runtime"),
            agent_id: ROOT_AGENT_ID.to_string(),
            path: ROOT_AGENT_PATH.to_string(),
            parent_path: None,
            role: ROOT_AGENT_ROLE.to_string(),
            model: model_name,
            context_window: model.and_then(pl_model::ModelInfo::resolved_context_window),
            usage,
            estimated_costs,
            has_unpriced_usage,
            updated_at: unix_seconds(),
        };
        self.record_agent_runtime_delta(session_id, &delta).await?;
        Ok(())
    }

    pub async fn save_setting(&self, key: &str, value: &str) -> Result<()> {
        use entities::app_setting;
        let now = unix_seconds();
        if let Some(existing) = app_setting::Entity::find_by_id(key.to_string())
            .one(&self.db)
            .await?
        {
            let mut active: app_setting::ActiveModel = existing.into();
            active.value = Set(value.to_string());
            active.updated_at = Set(now);
            active.update(&self.db).await?;
            return Ok(());
        }

        app_setting::ActiveModel {
            key: Set(key.to_string()),
            value: Set(value.to_string()),
            updated_at: Set(now),
        }
        .insert(&self.db)
        .await?;
        Ok(())
    }

    pub async fn load_setting(&self, key: &str) -> Result<Option<String>> {
        use entities::app_setting;
        Ok(app_setting::Entity::find_by_id(key.to_string())
            .one(&self.db)
            .await?
            .map(|setting| setting.value))
    }
}

fn runtime_snapshot_id(session_id: &str, agent_id: &str) -> String {
    format!("{session_id}:{agent_id}")
}

async fn start_plan_implementation_handoff_with_tx(
    tx: &sea_orm::DatabaseTransaction,
    interaction_id: &str,
    resolution: InteractionResolution,
) -> Result<PlanImplementationHandoffStart> {
    use entities::{interaction, session, session_handoff};
    let InteractionResolution::PlanConfirmation { decision, .. } = &resolution else {
        bail!("plan implementation handoff requires plan confirmation resolution");
    };
    if *decision != PlanConfirmationResolution::ImplementFreshContext {
        bail!("plan implementation handoff requires implementFreshContext decision");
    }

    let now = unix_seconds();
    let Some(interaction_row) = interaction::Entity::find_by_id(interaction_id.to_string())
        .one(tx)
        .await?
    else {
        bail!("interaction not found");
    };
    let current_interaction = interaction_record(interaction_row.clone())?;
    if current_interaction.kind != InteractionKind::PlanConfirmation {
        bail!("interaction resolution kind mismatch");
    }
    let InteractionPayload::PlanConfirmation {
        plan_id,
        content: stored_content,
    } = current_interaction.payload.clone()
    else {
        bail!("interaction payload mismatch");
    };

    let Some(origin_session_row) =
        session::Entity::find_by_id(current_interaction.scope.session_id.clone())
            .one(tx)
            .await?
    else {
        bail!("origin session not found");
    };
    let origin_session_before = session_record(origin_session_row.clone());

    if let Some(existing_handoff) = session_handoff::Entity::find()
        .filter(session_handoff::Column::OriginSessionId.eq(origin_session_before.id.clone()))
        .filter(session_handoff::Column::PlanId.eq(plan_id.clone()))
        .filter(session_handoff::Column::Kind.eq(SessionHandoffKind::PlanImplementation.as_str()))
        .one(tx)
        .await?
    {
        let Some(target_row) =
            session::Entity::find_by_id(existing_handoff.target_session_id.clone())
                .one(tx)
                .await?
        else {
            bail!("plan implementation target session not found");
        };
        let interaction = if current_interaction.status == InteractionStatus::Pending {
            resolve_interaction_row_with_tx(tx, interaction_row, resolution, now).await?
        } else {
            current_interaction
        };
        return Ok(PlanImplementationHandoffStart {
            origin_session: origin_session_before,
            target_session: session_record(target_row),
            handoff: session_handoff_record(existing_handoff),
            interaction,
            plan_id,
            plan_content: stored_content,
            plan_lifecycle_events: Vec::new(),
            should_start_run: false,
        });
    }

    if current_interaction.status != InteractionStatus::Pending {
        bail!("plan confirmation is already resolved and has no implementation handoff");
    }

    let resolved_interaction =
        resolve_interaction_row_with_tx(tx, interaction_row, resolution.clone(), now).await?;
    let target_session_row = session::ActiveModel {
        id: Set(new_id("session")),
        project_id: Set(origin_session_before.project_id.clone()),
        title: Set(non_empty_title("实施计划")),
        mode: Set(CompileMode::Auto.label().to_string()),
        created_at: Set(now),
        updated_at: Set(now),
        archived: Set(0),
        visibility: Set(SessionVisibility::Active.as_str().to_string()),
        instruction_snapshot_json: Set(None),
    }
    .insert(tx)
    .await?;

    let mut origin_active: session::ActiveModel = origin_session_row.into();
    origin_active.visibility = Set(SessionVisibility::HandoffOrigin.as_str().to_string());
    origin_active.updated_at = Set(now);
    origin_active.update(tx).await?;

    let handoff_row = session_handoff::ActiveModel {
        id: Set(new_id("handoff")),
        project_id: Set(origin_session_before.project_id.clone()),
        origin_session_id: Set(origin_session_before.id.clone()),
        target_session_id: Set(target_session_row.id.clone()),
        kind: Set(SessionHandoffKind::PlanImplementation.as_str().to_string()),
        plan_id: Set(plan_id.clone()),
        status: Set(SessionHandoffStatus::Running.as_str().to_string()),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(tx)
    .await?;

    let plan_lifecycle_events = plan_lifecycle_events(
        &plan_id,
        now,
        [
            PlanLifecycleState::Accepted,
            PlanLifecycleState::Implementing,
        ],
    );

    Ok(PlanImplementationHandoffStart {
        origin_session: origin_session_before,
        target_session: session_record(target_session_row),
        handoff: session_handoff_record(handoff_row),
        interaction: resolved_interaction,
        plan_id,
        plan_content: stored_content,
        plan_lifecycle_events,
        should_start_run: true,
    })
}

async fn resolve_interaction_row_with_tx(
    tx: &sea_orm::DatabaseTransaction,
    interaction_row: entities::interaction::Model,
    resolution: InteractionResolution,
    now: i64,
) -> Result<InteractionRequest> {
    let mut active: entities::interaction::ActiveModel = interaction_row.into();
    active.status = Set(InteractionStatus::Resolved.as_str().to_string());
    active.resolution_json = Set(Some(serde_json::to_string(&resolution)?));
    active.updated_at = Set(now);
    active.resolved_at = Set(Some(now));
    let updated = active.update(tx).await?;
    interaction_record(updated)
}

fn plan_lifecycle_events(
    plan_id: &str,
    now: i64,
    states: impl IntoIterator<Item = PlanLifecycleState>,
) -> Vec<PlanLifecycleEvent> {
    states
        .into_iter()
        .map(|state| PlanLifecycleEvent {
            plan_id: plan_id.to_string(),
            state,
            turn_id: None,
            reason: None,
            updated_at: now,
        })
        .collect()
}

fn timeline_event_models(events: &[TraceEvent]) -> Vec<entities::timeline_event::ActiveModel> {
    events
        .iter()
        .map(|event| {
            let payload = serde_json::to_string(&event.kind).unwrap_or_default();
            entities::timeline_event::ActiveModel {
                id: Set(new_timeline_event_id()),
                session_id: Set(event.session_id.clone()),
                sequence: Set(event.sequence as i64),
                created_at: Set(event.timestamp),
                kind: Set(trace_event_kind_label(&event.kind).to_string()),
                payload_json: Set(payload),
            }
        })
        .collect()
}

fn timeline_event_sequence_conflict() -> sea_orm::sea_query::OnConflict {
    use entities::timeline_event;
    sea_orm::sea_query::OnConflict::columns([
        timeline_event::Column::SessionId,
        timeline_event::Column::Sequence,
    ])
    .do_nothing()
    .to_owned()
}

async fn next_studio_event_sequence_with_tx(
    tx: &sea_orm::DatabaseTransaction,
    session_id: &str,
) -> Result<i64> {
    use entities::studio_event;
    let max_seq = studio_event::Entity::find()
        .filter(studio_event::Column::SessionId.eq(session_id.to_string()))
        .order_by_desc(studio_event::Column::Sequence)
        .one(tx)
        .await?
        .map(|row| row.sequence);
    Ok(max_seq.map(|sequence| sequence + 1).unwrap_or(0))
}

async fn insert_studio_event_with_tx(
    tx: &sea_orm::DatabaseTransaction,
    envelope: &StudioEventEnvelope,
) -> Result<()> {
    use entities::studio_event;
    let payload_json = serde_json::to_string(envelope)?;
    studio_event::ActiveModel {
        id: Set(envelope.event_id.clone()),
        project_id: Set(envelope.project_id.clone()),
        session_id: Set(envelope.session_id.clone()),
        turn_id: Set(envelope.turn_id.clone()),
        sequence: Set(envelope.sequence as i64),
        created_at: Set(envelope.created_at),
        kind: Set(studio_event_kind_label(&envelope.kind).to_string()),
        payload_json: Set(payload_json),
    }
    .insert(tx)
    .await?;
    Ok(())
}

async fn canonicalize_studio_event_with_connection<C>(
    conn: &C,
    mut envelope: StudioEventEnvelope,
) -> Result<StudioEventEnvelope>
where
    C: ConnectionTrait,
{
    if let StudioEventKind::MessagePartUpdated { part } = &mut envelope.kind {
        let existing_order =
            existing_message_part_order_with_connection(conn, &part.part_id).await?;
        part.order = existing_order.unwrap_or(envelope.sequence);
    }
    Ok(envelope)
}

async fn existing_message_part_order_with_connection<C>(
    conn: &C,
    part_id: &str,
) -> Result<Option<u64>>
where
    C: ConnectionTrait,
{
    use entities::message_part;
    Ok(message_part::Entity::find_by_id(part_id.to_string())
        .one(conn)
        .await?
        .map(|row| row.part_order as u64))
}

async fn upsert_agent_snapshot_with_tx(
    tx: &sea_orm::DatabaseTransaction,
    record: AgentSnapshotRecord,
) -> Result<()> {
    use entities::agent;
    if let Some(existing) = agent::Entity::find_by_id(record.id.clone()).one(tx).await? {
        let mut active: agent::ActiveModel = existing.into();
        active.session_id = Set(record.session_id);
        active.path = Set(record.path);
        active.parent_path = Set(record.parent_path);
        active.role = Set(record.role);
        active.task = Set(record.task);
        active.status = Set(record.status.as_str().to_string());
        active.summary = Set(record.summary);
        active.error = Set(record.error);
        active.reason = Set(record.reason);
        active.budget_limit_kind = Set(record
            .budget_limit_kind
            .map(|kind| kind.as_str().to_string()));
        active.budget_usage_json = Set(record
            .budget_usage
            .and_then(|usage| serde_json::to_string(&usage).ok()));
        active.depth = Set(record.depth);
        active.updated_at = Set(record.updated_at);
        active.update(tx).await?;
    } else {
        agent::ActiveModel {
            id: Set(record.id),
            session_id: Set(record.session_id),
            path: Set(record.path),
            parent_path: Set(record.parent_path),
            role: Set(record.role),
            task: Set(record.task),
            status: Set(record.status.as_str().to_string()),
            summary: Set(record.summary),
            error: Set(record.error),
            reason: Set(record.reason),
            budget_limit_kind: Set(record
                .budget_limit_kind
                .map(|kind| kind.as_str().to_string())),
            budget_usage_json: Set(record
                .budget_usage
                .and_then(|usage| serde_json::to_string(&usage).ok())),
            depth: Set(record.depth),
            updated_at: Set(record.updated_at),
        }
        .insert(tx)
        .await?;
    }
    Ok(())
}

async fn insert_agent_event_with_tx(
    tx: &sea_orm::DatabaseTransaction,
    record: AgentTimelineEventRecord,
) -> Result<()> {
    use entities::agent_event;
    agent_event::ActiveModel {
        id: Set(record.event_id),
        session_id: Set(record.session_id),
        sequence: Set(record.sequence),
        kind: Set(record.kind),
        agent_id: Set(record.agent_id),
        path: Set(record.path),
        parent_path: Set(record.parent_path),
        payload_json: Set(record.payload_json),
        created_at: Set(record.created_at),
    }
    .insert(tx)
    .await?;
    Ok(())
}

async fn upsert_studio_message_with_tx(
    tx: &sea_orm::DatabaseTransaction,
    message: &StudioMessage,
    sequence: i64,
) -> Result<()> {
    use entities::studio_message;
    let metadata_json = serde_json::to_string(&message.metadata)?;
    if let Some(existing) = studio_message::Entity::find_by_id(message.message_id.clone())
        .one(tx)
        .await?
    {
        if existing.sequence > sequence {
            return Ok(());
        }
        let mut active: studio_message::ActiveModel = existing.into();
        active.session_id = Set(message.session_id.clone());
        active.turn_id = Set(message.turn_id.clone());
        active.role = Set(studio_message_role_label(message.role).to_string());
        active.status = Set(studio_message_status_label(message.status).to_string());
        active.created_at = Set(message.created_at);
        active.updated_at = Set(message.updated_at);
        active.completed_at = Set(message.completed_at);
        active.error = Set(message.error.clone());
        active.metadata_json = Set(metadata_json);
        active.sequence = Set(sequence);
        active.update(tx).await?;
    } else {
        studio_message::ActiveModel {
            id: Set(message.message_id.clone()),
            session_id: Set(message.session_id.clone()),
            turn_id: Set(message.turn_id.clone()),
            role: Set(studio_message_role_label(message.role).to_string()),
            status: Set(studio_message_status_label(message.status).to_string()),
            created_at: Set(message.created_at),
            updated_at: Set(message.updated_at),
            completed_at: Set(message.completed_at),
            error: Set(message.error.clone()),
            metadata_json: Set(metadata_json),
            sequence: Set(sequence),
        }
        .insert(tx)
        .await?;
    }
    Ok(())
}

async fn delete_studio_message_with_tx(
    tx: &sea_orm::DatabaseTransaction,
    message_id: &str,
) -> Result<()> {
    use entities::{message_part, studio_message};
    message_part::Entity::delete_many()
        .filter(message_part::Column::MessageId.eq(message_id.to_string()))
        .exec(tx)
        .await?;
    studio_message::Entity::delete_by_id(message_id.to_string())
        .exec(tx)
        .await?;
    Ok(())
}

async fn upsert_message_part_with_tx(
    tx: &sea_orm::DatabaseTransaction,
    part: &StudioPart,
    sequence: i64,
) -> Result<()> {
    use entities::message_part;
    let attachments_json = serde_json::to_string(&part.attachments)?;
    let tool_json = optional_json_string(&part.tool)?;
    let agent_json = optional_json_string(&part.agent)?;
    let inference_json = optional_json_string(&part.inference)?;
    let plan_json = optional_json_string(&part.plan)?;
    let file_json = optional_json_string(&part.file)?;
    let usage_json = optional_json_string(&part.usage)?;
    if let Some(existing) = message_part::Entity::find_by_id(part.part_id.clone())
        .one(tx)
        .await?
    {
        if existing.sequence > sequence {
            return Ok(());
        }
        let mut active: message_part::ActiveModel = existing.into();
        active.message_id = Set(part.message_id.clone());
        active.session_id = Set(part.session_id.clone());
        active.turn_id = Set(part.turn_id.clone());
        active.part_type = Set(studio_part_type_label(part.part_type).to_string());
        active.part_order = Set(part.order as i64);
        active.status = Set(studio_part_status_label(part.status).to_string());
        active.created_at = Set(part.created_at);
        active.updated_at = Set(part.updated_at);
        active.completed_at = Set(part.completed_at);
        active.error = Set(part.error.clone());
        active.text_channel = Set(part
            .text_channel
            .map(studio_text_channel_label)
            .map(str::to_string));
        active.text = Set(part.text.clone());
        active.attachments_json = Set(attachments_json);
        active.tool_json = Set(tool_json);
        active.agent_json = Set(agent_json);
        active.inference_json = Set(inference_json);
        active.plan_json = Set(plan_json);
        active.file_json = Set(file_json);
        active.usage_json = Set(usage_json);
        active.synthetic = Set(i32::from(part.synthetic));
        active.ignored = Set(i32::from(part.ignored));
        active.sequence = Set(sequence);
        active.update(tx).await?;
    } else {
        message_part::ActiveModel {
            id: Set(part.part_id.clone()),
            message_id: Set(part.message_id.clone()),
            session_id: Set(part.session_id.clone()),
            turn_id: Set(part.turn_id.clone()),
            part_type: Set(studio_part_type_label(part.part_type).to_string()),
            part_order: Set(part.order as i64),
            status: Set(studio_part_status_label(part.status).to_string()),
            created_at: Set(part.created_at),
            updated_at: Set(part.updated_at),
            completed_at: Set(part.completed_at),
            error: Set(part.error.clone()),
            text_channel: Set(part
                .text_channel
                .map(studio_text_channel_label)
                .map(str::to_string)),
            text: Set(part.text.clone()),
            attachments_json: Set(attachments_json),
            tool_json: Set(tool_json),
            agent_json: Set(agent_json),
            inference_json: Set(inference_json),
            plan_json: Set(plan_json),
            file_json: Set(file_json),
            usage_json: Set(usage_json),
            synthetic: Set(i32::from(part.synthetic)),
            ignored: Set(i32::from(part.ignored)),
            sequence: Set(sequence),
        }
        .insert(tx)
        .await?;
    }
    Ok(())
}

async fn delete_message_part_with_tx(
    tx: &sea_orm::DatabaseTransaction,
    part_id: &str,
) -> Result<()> {
    use entities::message_part;
    message_part::Entity::delete_by_id(part_id.to_string())
        .exec(tx)
        .await?;
    Ok(())
}

fn optional_json_string<T: serde::Serialize>(value: &Option<T>) -> Result<Option<String>> {
    Ok(value.as_ref().map(serde_json::to_string).transpose()?)
}

async fn record_agent_runtime_delta_with_tx(
    tx: &sea_orm::DatabaseTransaction,
    session_id: &str,
    delta: &AgentRuntimeDelta,
) -> Result<bool> {
    use entities::{agent_runtime_event, agent_runtime_snapshot};

    let exists = agent_runtime_event::Entity::find()
        .filter(agent_runtime_event::Column::SessionId.eq(session_id.to_string()))
        .filter(agent_runtime_event::Column::InferenceId.eq(delta.inference_id.clone()))
        .one(tx)
        .await?
        .is_some();
    if exists {
        return Ok(false);
    }

    let costs_json = costs_to_json(&delta.estimated_costs);
    agent_runtime_event::ActiveModel {
        id: Set(new_id("agent-runtime-event")),
        session_id: Set(session_id.to_string()),
        inference_id: Set(delta.inference_id.clone()),
        agent_id: Set(delta.agent_id.clone()),
        path: Set(delta.path.clone()),
        parent_path: Set(delta.parent_path.clone()),
        role: Set(delta.role.clone()),
        model: Set(delta.model.clone()),
        context_window: Set(delta.context_window.map(|value| value as i64)),
        prompt_tokens: Set(delta.usage.prompt_tokens as i64),
        completion_tokens: Set(delta.usage.completion_tokens as i64),
        cached_prompt_tokens: Set(delta.usage.cached_prompt_tokens as i64),
        total_tokens: Set(delta.usage.total_tokens as i64),
        estimated_costs_json: Set(costs_json.clone()),
        has_unpriced_usage: Set(i32::from(delta.has_unpriced_usage)),
        created_at: Set(delta.updated_at),
    }
    .insert(tx)
    .await?;

    let existing = agent_runtime_snapshot::Entity::find()
        .filter(agent_runtime_snapshot::Column::SessionId.eq(session_id.to_string()))
        .filter(agent_runtime_snapshot::Column::AgentId.eq(delta.agent_id.clone()))
        .one(tx)
        .await?;
    if let Some(row) = existing {
        let mut costs = crate::studio::mappers::costs_from_json(&row.estimated_costs_json);
        merge_costs(&mut costs, &delta.estimated_costs);
        let prompt_tokens = row.prompt_tokens + delta.usage.prompt_tokens as i64;
        let completion_tokens = row.completion_tokens + delta.usage.completion_tokens as i64;
        let cached_prompt_tokens =
            row.cached_prompt_tokens + delta.usage.cached_prompt_tokens as i64;
        let total_tokens = row.total_tokens + delta.usage.total_tokens as i64;
        let has_unpriced_usage = row.has_unpriced_usage != 0 || delta.has_unpriced_usage;
        let mut active: agent_runtime_snapshot::ActiveModel = row.into();
        active.path = Set(delta.path.clone());
        active.parent_path = Set(delta.parent_path.clone());
        active.role = Set(delta.role.clone());
        active.model = Set(delta.model.clone());
        active.context_window = Set(delta.context_window.map(|value| value as i64));
        active.latest_context_tokens = Set(delta.usage.prompt_tokens as i64);
        active.prompt_tokens = Set(prompt_tokens);
        active.completion_tokens = Set(completion_tokens);
        active.cached_prompt_tokens = Set(cached_prompt_tokens);
        active.total_tokens = Set(total_tokens);
        active.estimated_costs_json = Set(costs_to_json(&costs));
        active.has_unpriced_usage = Set(i32::from(has_unpriced_usage));
        active.updated_at = Set(delta.updated_at);
        active.update(tx).await?;
    } else {
        agent_runtime_snapshot::ActiveModel {
            id: Set(runtime_snapshot_id(session_id, &delta.agent_id)),
            session_id: Set(session_id.to_string()),
            agent_id: Set(delta.agent_id.clone()),
            path: Set(delta.path.clone()),
            parent_path: Set(delta.parent_path.clone()),
            role: Set(delta.role.clone()),
            model: Set(delta.model.clone()),
            context_window: Set(delta.context_window.map(|value| value as i64)),
            latest_context_tokens: Set(delta.usage.prompt_tokens as i64),
            prompt_tokens: Set(delta.usage.prompt_tokens as i64),
            completion_tokens: Set(delta.usage.completion_tokens as i64),
            cached_prompt_tokens: Set(delta.usage.cached_prompt_tokens as i64),
            total_tokens: Set(delta.usage.total_tokens as i64),
            estimated_costs_json: Set(costs_json),
            has_unpriced_usage: Set(i32::from(delta.has_unpriced_usage)),
            updated_at: Set(delta.updated_at),
        }
        .insert(tx)
        .await?;
    }

    rebuild_session_runtime_from_agent_snapshots_with_tx(tx, session_id).await?;
    Ok(true)
}

async fn rebuild_session_runtime_from_agent_snapshots_with_tx(
    tx: &sea_orm::DatabaseTransaction,
    session_id: &str,
) -> Result<()> {
    use entities::{agent_runtime_snapshot, session_runtime_snapshot};

    let rows = agent_runtime_snapshot::Entity::find()
        .filter(agent_runtime_snapshot::Column::SessionId.eq(session_id.to_string()))
        .all(tx)
        .await?;
    if rows.is_empty() {
        return Ok(());
    }
    let aggregate = aggregate_runtime_usage(
        "unknown",
        rows.into_iter().map(agent_runtime_snapshot_record),
    );
    let (currency, estimated_cost) = if aggregate.estimated_costs.len() == 1 {
        (
            Some(aggregate.estimated_costs[0].currency.clone()),
            Some(aggregate.estimated_costs[0].amount),
        )
    } else {
        (None, None)
    };
    let costs_json = costs_to_json(&aggregate.estimated_costs);

    if let Some(row) = session_runtime_snapshot::Entity::find_by_id(session_id.to_string())
        .one(tx)
        .await?
    {
        let mut active: session_runtime_snapshot::ActiveModel = row.into();
        active.model = Set(aggregate.model);
        active.context_window = Set(aggregate.context_window.map(|value| value as i64));
        active.latest_context_tokens = Set(aggregate.latest_context_tokens as i64);
        active.prompt_tokens = Set(aggregate.prompt_tokens as i64);
        active.completion_tokens = Set(aggregate.completion_tokens as i64);
        active.cached_prompt_tokens = Set(aggregate.cached_prompt_tokens as i64);
        active.total_tokens = Set(aggregate.total_tokens as i64);
        active.currency = Set(currency);
        active.estimated_cost = Set(estimated_cost);
        active.estimated_costs_json = Set(costs_json);
        active.has_unpriced_usage = Set(i32::from(aggregate.has_unpriced_usage));
        active.updated_at = Set(aggregate.updated_at);
        active.update(tx).await?;
    } else {
        session_runtime_snapshot::ActiveModel {
            session_id: Set(session_id.to_string()),
            model: Set(aggregate.model),
            context_window: Set(aggregate.context_window.map(|value| value as i64)),
            latest_context_tokens: Set(aggregate.latest_context_tokens as i64),
            prompt_tokens: Set(aggregate.prompt_tokens as i64),
            completion_tokens: Set(aggregate.completion_tokens as i64),
            cached_prompt_tokens: Set(aggregate.cached_prompt_tokens as i64),
            total_tokens: Set(aggregate.total_tokens as i64),
            currency: Set(currency),
            estimated_cost: Set(estimated_cost),
            estimated_costs_json: Set(costs_json),
            has_unpriced_usage: Set(i32::from(aggregate.has_unpriced_usage)),
            updated_at: Set(aggregate.updated_at),
        }
        .insert(tx)
        .await?;
    }
    Ok(())
}

async fn apply_studio_event_projection_with_tx(
    tx: &sea_orm::DatabaseTransaction,
    envelope: &StudioEventEnvelope,
) -> Result<()> {
    match &envelope.kind {
        StudioEventKind::MessageUpdated { message } => {
            upsert_studio_message_with_tx(tx, message, envelope.sequence as i64).await?;
        }
        StudioEventKind::MessageRemoved { message_id } => {
            delete_studio_message_with_tx(tx, message_id).await?;
        }
        StudioEventKind::MessagePartUpdated { part } => {
            upsert_message_part_with_tx(tx, part, envelope.sequence as i64).await?;
        }
        StudioEventKind::MessagePartRemoved { part_id, .. } => {
            delete_message_part_with_tx(tx, part_id).await?;
        }
        StudioEventKind::MessagePartDelta { .. } => {
            bail!("messagePartDelta is live-only and must not be projected");
        }
        StudioEventKind::SkillActivated { activation } => {
            if let Some(session_id) = envelope.session_id.as_deref() {
                upsert_session_skill_with_tx(tx, session_id, activation).await?;
            }
        }
        StudioEventKind::AgentChanged { agent } => {
            if let Some(session_id) = envelope.session_id.as_deref()
                && let Ok(AgentEvent::AgentStateChanged {
                    id,
                    path,
                    parent_path,
                    role,
                    task,
                    status,
                    summary,
                    depth,
                    error,
                    reason,
                    budget_limit_kind,
                    budget_usage,
                    updated_at,
                }) = serde_json::from_value::<AgentEvent>(agent.payload.clone())
            {
                upsert_agent_snapshot_with_tx(
                    tx,
                    AgentSnapshotRecord {
                        id,
                        session_id: session_id.to_string(),
                        path,
                        parent_path,
                        role,
                        task,
                        status,
                        summary,
                        depth: depth as i32,
                        error,
                        reason,
                        budget_limit_kind,
                        budget_usage,
                        runtime_usage: None,
                        updated_at,
                    },
                )
                .await?;
            }
        }
        StudioEventKind::AgentTimelineChanged { event } => {
            if let Some(session_id) = envelope.session_id.as_deref()
                && let Some(record) =
                    agent_timeline_event_record_from_payload(session_id, envelope, &event.payload)
            {
                insert_agent_event_with_tx(tx, record).await?;
            }
        }
        StudioEventKind::SessionRuntimeChanged { runtime } => {
            if let Some(session_id) = envelope.session_id.as_deref()
                && let Ok(AgentEvent::AgentRuntimeUpdated { delta }) =
                    serde_json::from_value::<AgentEvent>(runtime.payload.clone())
            {
                record_agent_runtime_delta_with_tx(tx, session_id, &delta).await?;
            }
        }
        StudioEventKind::InteractionChanged { .. }
        | StudioEventKind::PlanLifecycleChanged { .. }
        | StudioEventKind::TurnChanged { .. }
        | StudioEventKind::SessionHandoffChanged { .. }
        | StudioEventKind::SessionListChanged { .. }
        | StudioEventKind::McpHealthChanged { .. }
        | StudioEventKind::LspHealthChanged { .. }
        | StudioEventKind::Stale { .. } => {}
    }
    Ok(())
}

fn agent_timeline_event_record_from_payload(
    session_id: &str,
    envelope: &StudioEventEnvelope,
    payload: &serde_json::Value,
) -> Option<AgentTimelineEventRecord> {
    let event = serde_json::from_value::<AgentEvent>(payload.clone()).ok()?;
    let (kind, agent_id, path, parent_path) = match event {
        AgentEvent::AgentStateChanged {
            id,
            path,
            parent_path,
            ..
        } => ("agentStatus".to_string(), Some(id), Some(path), parent_path),
        AgentEvent::CollabAgentSpawnBegin { sender_path, .. } => {
            ("spawnBegin".to_string(), None, Some(sender_path), None)
        }
        AgentEvent::CollabAgentSpawnEnd { agent_id, path, .. } => {
            ("spawnEnd".to_string(), agent_id, path, None)
        }
        AgentEvent::CollabAgentInteractionBegin {
            receiver_path,
            sender_path,
            ..
        } => (
            "interactionBegin".to_string(),
            None,
            Some(receiver_path),
            Some(sender_path),
        ),
        AgentEvent::CollabAgentInteractionEnd {
            receiver_path,
            sender_path,
            ..
        } => (
            "interactionEnd".to_string(),
            None,
            Some(receiver_path),
            Some(sender_path),
        ),
        AgentEvent::CollabWaitingBegin { sender_path, .. } => {
            ("waitingBegin".to_string(), None, Some(sender_path), None)
        }
        AgentEvent::CollabWaitingEnd { sender_path, .. } => {
            ("waitingEnd".to_string(), None, Some(sender_path), None)
        }
        AgentEvent::CollabCloseBegin {
            receiver_path,
            sender_path,
            ..
        } => (
            "closeBegin".to_string(),
            None,
            Some(receiver_path),
            Some(sender_path),
        ),
        AgentEvent::CollabCloseEnd {
            receiver_path,
            sender_path,
            ..
        } => (
            "closeEnd".to_string(),
            None,
            Some(receiver_path),
            Some(sender_path),
        ),
        AgentEvent::TimelineItemStarted { .. }
        | AgentEvent::TimelineItemDelta { .. }
        | AgentEvent::TimelineItemCompleted { .. }
        | AgentEvent::TimelineItemFailed { .. }
        | AgentEvent::InteractionChanged { .. }
        | AgentEvent::AgentRuntimeUpdated { .. }
        | AgentEvent::SkillActivated { .. }
        | AgentEvent::TurnInterrupted { .. }
        | AgentEvent::TurnBudgetLimited { .. }
        | AgentEvent::Done
        | AgentEvent::Error { .. } => return None,
    };
    Some(AgentTimelineEventRecord {
        event_id: envelope.event_id.clone(),
        session_id: session_id.to_string(),
        sequence: envelope.sequence as i64,
        kind,
        agent_id,
        path,
        parent_path,
        payload_json: serde_json::to_string(payload).ok()?,
        created_at: envelope.created_at,
    })
}

fn studio_event_kind_label(kind: &StudioEventKind) -> &'static str {
    match kind {
        StudioEventKind::TurnChanged { .. } => "TurnChanged",
        StudioEventKind::MessageUpdated { .. } => "MessageUpdated",
        StudioEventKind::MessageRemoved { .. } => "MessageRemoved",
        StudioEventKind::MessagePartUpdated { .. } => "MessagePartUpdated",
        StudioEventKind::MessagePartRemoved { .. } => "MessagePartRemoved",
        StudioEventKind::MessagePartDelta { .. } => "MessagePartDelta",
        StudioEventKind::InteractionChanged { .. } => "InteractionChanged",
        StudioEventKind::AgentChanged { .. } => "AgentChanged",
        StudioEventKind::AgentTimelineChanged { .. } => "AgentTimelineChanged",
        StudioEventKind::SessionRuntimeChanged { .. } => "SessionRuntimeChanged",
        StudioEventKind::SkillActivated { .. } => "SkillActivated",
        StudioEventKind::PlanLifecycleChanged { .. } => "PlanLifecycleChanged",
        StudioEventKind::SessionHandoffChanged { .. } => "SessionHandoffChanged",
        StudioEventKind::SessionListChanged { .. } => "SessionListChanged",
        StudioEventKind::McpHealthChanged { .. } => "McpHealthChanged",
        StudioEventKind::LspHealthChanged { .. } => "LspHealthChanged",
        StudioEventKind::Stale { .. } => "Stale",
    }
}

fn is_terminal_turn_status(status: StudioTurnStatus) -> bool {
    matches!(
        status,
        StudioTurnStatus::Completed | StudioTurnStatus::Failed | StudioTurnStatus::Cancelled
    )
}

async fn upsert_session_skill_events_with_tx(
    tx: &sea_orm::DatabaseTransaction,
    timeline_events: &[TraceEvent],
) -> Result<()> {
    for event in timeline_events {
        if let TraceEventKind::SkillActivated { activation } = &event.kind {
            upsert_session_skill_with_tx(tx, &event.session_id, activation).await?;
        }
    }
    Ok(())
}

async fn upsert_session_skill_with_tx(
    tx: &sea_orm::DatabaseTransaction,
    session_id: &str,
    activation: &SkillActivation,
) -> Result<()> {
    use entities::session_skill;
    let skill_name_key = activation.name.to_ascii_lowercase();
    let id = session_skill_id(session_id, &activation.name);
    let existing = session_skill::Entity::find()
        .filter(session_skill::Column::SessionId.eq(session_id.to_string()))
        .filter(session_skill::Column::SkillNameKey.eq(skill_name_key.clone()))
        .one(tx)
        .await?;
    if let Some(row) = existing {
        let mut active: session_skill::ActiveModel = row.into();
        active.skill_name = Set(activation.name.clone());
        active.source = Set(activation.source.clone());
        active.path = Set(activation.path.clone());
        active.last_turn_id = Set(activation.turn_id.clone());
        active.last_tool_call_id = Set(activation.tool_call_id.clone());
        active.updated_at = Set(activation.activated_at);
        active.update(tx).await?;
    } else {
        session_skill::ActiveModel {
            id: Set(id),
            session_id: Set(session_id.to_string()),
            skill_name: Set(activation.name.clone()),
            skill_name_key: Set(skill_name_key),
            source: Set(activation.source.clone()),
            path: Set(activation.path.clone()),
            first_turn_id: Set(activation.turn_id.clone()),
            last_turn_id: Set(activation.turn_id.clone()),
            last_tool_call_id: Set(activation.tool_call_id.clone()),
            activated_at: Set(activation.activated_at),
            updated_at: Set(activation.activated_at),
        }
        .insert(tx)
        .await?;
    }
    Ok(())
}

fn session_skill_id(session_id: &str, skill_name: &str) -> String {
    format!("{session_id}:{}", skill_name.to_ascii_lowercase())
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

const MAX_IMAGE_SIDE: u32 = 2000;
const MAX_BASE64_IMAGE_BYTES: usize = 5 * 1024 * 1024;
const JPEG_COMPRESSION_QUALITIES: [u8; 6] = [85, 75, 65, 55, 45, 35];
const JPEG_COMPRESSION_MAX_SIDES: [u32; 6] = [2000, 1600, 1280, 1024, 768, 512];

struct NormalizedImageAttachment {
    media_type: &'static str,
    bytes: Vec<u8>,
    dimensions: (u32, u32),
}

fn decode_image_data_url(data_url: &str) -> Result<(&'static str, Vec<u8>)> {
    let (header, data) = data_url
        .split_once(',')
        .context("image attachment must be a data URL")?;
    let media_type = header
        .strip_prefix("data:")
        .and_then(|value| value.strip_suffix(";base64"))
        .context("image attachment must be base64 encoded")?;
    let media_type = normalize_image_media_type(media_type)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .with_context(|| "invalid base64 image attachment")?;
    Ok((media_type, bytes))
}

fn normalize_image_media_type(media_type: &str) -> Result<&'static str> {
    match media_type {
        "image/png" => Ok("image/png"),
        "image/jpeg" | "image/jpg" => Ok("image/jpeg"),
        "image/webp" => Ok("image/webp"),
        other => bail!("unsupported image attachment media type: {other}"),
    }
}

fn extension_for_media_type(media_type: &str) -> Result<&'static str> {
    match media_type {
        "image/png" => Ok("png"),
        "image/jpeg" => Ok("jpg"),
        "image/webp" => Ok("webp"),
        other => bail!("unsupported image attachment media type: {other}"),
    }
}

fn normalize_image_attachment(
    media_type: &'static str,
    bytes: Vec<u8>,
    decoded_image: image::DynamicImage,
) -> Result<NormalizedImageAttachment> {
    let dimensions = decoded_image.dimensions();
    if image_within_limits(&bytes, dimensions) {
        return Ok(NormalizedImageAttachment {
            media_type,
            bytes,
            dimensions,
        });
    }

    let (compressed, dimensions) = compress_image_attachment(&decoded_image)?;
    Ok(NormalizedImageAttachment {
        media_type: "image/jpeg",
        bytes: compressed,
        dimensions,
    })
}

fn image_within_limits(bytes: &[u8], dimensions: (u32, u32)) -> bool {
    dimensions.0 <= MAX_IMAGE_SIDE
        && dimensions.1 <= MAX_IMAGE_SIDE
        && base64_encoded_len(bytes.len()) <= MAX_BASE64_IMAGE_BYTES
}

fn base64_encoded_len(byte_len: usize) -> usize {
    byte_len.div_ceil(3) * 4
}

fn compress_image_attachment(decoded_image: &image::DynamicImage) -> Result<(Vec<u8>, (u32, u32))> {
    for max_side in JPEG_COMPRESSION_MAX_SIDES {
        let candidate = if decoded_image.width() > max_side || decoded_image.height() > max_side {
            decoded_image.thumbnail(max_side, max_side)
        } else {
            decoded_image.clone()
        };
        let dimensions = candidate.dimensions();
        for quality in JPEG_COMPRESSION_QUALITIES {
            let bytes = encode_jpeg(&candidate, quality)?;
            if image_within_limits(&bytes, dimensions) {
                return Ok((bytes, dimensions));
            }
        }
    }
    bail!("image attachment is too large after compression")
}

fn encode_jpeg(image: &image::DynamicImage, quality: u8) -> Result<Vec<u8>> {
    let rgb = image.to_rgb8();
    let mut bytes = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, quality);
    encoder
        .encode(
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            image::ExtendedColorType::Rgb8,
        )
        .with_context(|| "failed to compress image attachment")?;
    Ok(bytes)
}

async fn materialize_attachments(
    records: Vec<AttachmentRecord>,
) -> Result<Vec<MaterializedAttachment>> {
    let mut materialized = Vec::with_capacity(records.len());
    for record in records {
        let bytes = tokio::fs::read(PathBuf::from(&record.storage_path))
            .await
            .with_context(|| format!("failed to read attachment {}", record.id))?;
        materialized.push(MaterializedAttachment {
            attachment_id: record.id,
            media_type: record.media_type,
            filename: record.filename,
            data: base64::engine::general_purpose::STANDARD.encode(bytes),
            byte_size: record.byte_size,
            width: record.width,
            height: record.height,
        });
    }
    Ok(materialized)
}

pub(crate) fn timeline_attachment(record: &AttachmentRecord) -> TimelineAttachment {
    TimelineAttachment {
        id: record.id.clone(),
        media_type: record.media_type.clone(),
        filename: record.filename.clone(),
        width: record.width,
        height: record.height,
        byte_size: record.byte_size,
        data_url: None,
    }
}

pub fn studio_attachment(record: &AttachmentRecord) -> StudioAttachment {
    StudioAttachment {
        id: record.id.clone(),
        media_type: record.media_type.clone(),
        filename: record.filename.clone(),
        width: record.width,
        height: record.height,
        byte_size: record.byte_size,
        data_url: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_protocol::{
        StudioEventEnvelope, StudioEventKind, StudioMessage, StudioMessageRole,
        StudioMessageStatus, StudioPart, StudioPartDelta, StudioPartDeltaField, StudioPartStatus,
        StudioPartType, StudioTextChannel,
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn oversized_image_attachment_is_resized_and_compressed() {
        let image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            MAX_IMAGE_SIDE + 100,
            10,
            image::Rgba([240, 64, 32, 255]),
        ));
        let mut cursor = std::io::Cursor::new(Vec::new());
        image
            .write_to(&mut cursor, image::ImageFormat::Png)
            .unwrap();
        let bytes = cursor.into_inner();

        let normalized = normalize_image_attachment("image/png", bytes, image).unwrap();

        assert_eq!(normalized.media_type, "image/jpeg");
        assert!(normalized.dimensions.0 <= MAX_IMAGE_SIDE);
        assert!(normalized.dimensions.1 <= MAX_IMAGE_SIDE);
        assert!(base64_encoded_len(normalized.bytes.len()) <= MAX_BASE64_IMAGE_BYTES);
    }

    #[tokio::test]
    async fn message_part_snapshot_projection_preserves_first_order() {
        let store = StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project("C:/work/alpha").await.unwrap();
        let session = store
            .create_session(&project.id, "Conversation", CompileMode::Auto)
            .await
            .unwrap();
        let message = StudioMessage {
            message_id: "turn-1-assistant".to_string(),
            session_id: session.id.clone(),
            turn_id: "turn-1".to_string(),
            role: StudioMessageRole::Assistant,
            status: StudioMessageStatus::Streaming,
            created_at: 10,
            updated_at: 10,
            completed_at: None,
            error: None,
            metadata: serde_json::json!({}),
        };
        store
            .append_studio_event(StudioEventEnvelope {
                event_id: "studio-event-1".to_string(),
                project_id: Some(project.id.clone()),
                session_id: Some(session.id.clone()),
                turn_id: Some("turn-1".to_string()),
                sequence: 0,
                created_at: 10,
                kind: StudioEventKind::MessageUpdated {
                    message: Box::new(message),
                },
            })
            .await
            .unwrap();
        let part = StudioPart {
            part_id: "turn-1-final".to_string(),
            message_id: "turn-1-assistant".to_string(),
            session_id: session.id.clone(),
            turn_id: "turn-1".to_string(),
            part_type: StudioPartType::Text,
            order: 999,
            status: StudioPartStatus::Streaming,
            created_at: 10,
            updated_at: 10,
            completed_at: None,
            error: None,
            text_channel: Some(StudioTextChannel::Final),
            text: String::new(),
            attachments: Vec::new(),
            tool: None,
            agent: None,
            inference: None,
            plan: None,
            file: None,
            usage: None,
            synthetic: false,
            ignored: false,
        };
        let first_part = store
            .append_studio_event(StudioEventEnvelope {
                event_id: "studio-event-2".to_string(),
                project_id: Some(project.id.clone()),
                session_id: Some(session.id.clone()),
                turn_id: Some("turn-1".to_string()),
                sequence: 0,
                created_at: 10,
                kind: StudioEventKind::MessagePartUpdated {
                    part: Box::new(part.clone()),
                },
            })
            .await
            .unwrap();
        let mut completed_part = part;
        completed_part.order = 777;
        completed_part.status = StudioPartStatus::Completed;
        completed_part.text = "hello".to_string();
        completed_part.updated_at = 11;
        completed_part.completed_at = Some(11);
        let second_part = store
            .append_studio_event(StudioEventEnvelope {
                event_id: "studio-event-3".to_string(),
                project_id: Some(project.id),
                session_id: Some(session.id.clone()),
                turn_id: Some("turn-1".to_string()),
                sequence: 0,
                created_at: 11,
                kind: StudioEventKind::MessagePartUpdated {
                    part: Box::new(completed_part),
                },
            })
            .await
            .unwrap();

        let StudioEventKind::MessagePartUpdated { part } = first_part.kind else {
            panic!("expected first part snapshot");
        };
        assert_eq!(part.order, 1);
        let StudioEventKind::MessagePartUpdated { part } = second_part.kind else {
            panic!("expected second part snapshot");
        };
        assert_eq!(part.order, 1);

        let parts = store.load_message_parts(&session.id).await.unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].part.order, 1);
        assert_eq!(parts[0].part.text, "hello");
        assert_eq!(parts[0].sequence, 2);
    }

    #[tokio::test]
    async fn message_part_delta_is_not_durable() {
        let store = StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project("C:/work/beta").await.unwrap();
        let session = store
            .create_session(&project.id, "Live", CompileMode::Auto)
            .await
            .unwrap();
        let err = store
            .append_studio_event(StudioEventEnvelope {
                event_id: "studio-event-live".to_string(),
                project_id: Some(project.id),
                session_id: Some(session.id),
                turn_id: Some("turn-1".to_string()),
                sequence: 0,
                created_at: 10,
                kind: StudioEventKind::MessagePartDelta {
                    delta: StudioPartDelta {
                        session_id: "session-1".to_string(),
                        message_id: "message-1".to_string(),
                        part_id: "part-1".to_string(),
                        field: StudioPartDeltaField::Text,
                        delta: "live".to_string(),
                        chunk_index: None,
                    },
                },
            })
            .await
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("messagePartDelta is live-only and must not be persisted")
        );
    }
}
