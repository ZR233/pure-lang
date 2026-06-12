use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use pl_protocol::{
    AgentRuntimeDelta, InteractionRequest, Message, RuntimeUsageSnapshot, SkillActivation,
    TraceEventKind,
};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectOptions, ConnectionTrait, Database,
    DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, QuerySelect, Statement,
    TransactionTrait,
};

use crate::runtime_usage::{
    ROOT_AGENT_ID, ROOT_AGENT_PATH, ROOT_AGENT_ROLE, aggregate_runtime_usage, cost_for_usage,
    merge_costs, token_usage_snapshot,
};
use crate::studio::entities;
use crate::studio::ids::{new_id, new_timeline_event_id, unix_seconds};
use crate::studio::mappers::{
    agent_runtime_snapshot_record, agent_snapshot_record, agent_timeline_event_record,
    costs_to_json, interaction_record, message_to_row_parts, project_record, row_to_message,
    session_record, session_runtime_record, session_skill_record, timeline_event_record,
    trace_event_kind_label,
};
use crate::studio::paths::{prepare_database_switch, project_name, sqlite_url};
use crate::studio::records::{
    AgentSnapshotRecord, AgentTimelineEventRecord, ProjectRecord, SessionRecord,
    SessionRuntimeRecord, SessionSkillRecord, TimelineEventRecord,
};
use crate::studio::store_support::{
    configure_sqlite, insert_message_with_tx, non_empty_title, run_migrations,
    touch_session_with_tx,
};
use crate::{CompileMode, CoreSession, InstructionSnapshot, TraceEvent, TurnResult};
#[derive(Clone)]
pub struct StudioStore {
    db: DatabaseConnection,
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
            .max_connections(5)
            .min_connections(1)
            .connect_timeout(Duration::from_secs(8))
            .acquire_timeout(Duration::from_secs(8))
            .sqlx_logging(false);
        let db = Database::connect(options).await?;
        configure_sqlite(&db).await?;
        run_migrations(&db).await?;
        Ok(Self { db })
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
            project, session, session_runtime_snapshot, session_skill, timeline_event,
            tool_approval,
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

    pub async fn append_timeline_events(&self, events: &[TraceEvent]) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }
        use entities::timeline_event;
        let models: Vec<timeline_event::ActiveModel> = events
            .iter()
            .map(|event| {
                let payload = serde_json::to_string(&event.kind).unwrap_or_default();
                timeline_event::ActiveModel {
                    id: Set(new_timeline_event_id()),
                    session_id: Set(event.session_id.clone()),
                    sequence: Set(event.sequence as i64),
                    created_at: Set(event.timestamp),
                    kind: Set(trace_event_kind_label(&event.kind).to_string()),
                    payload_json: Set(payload),
                }
            })
            .collect();
        timeline_event::Entity::insert_many(models)
            .exec(&self.db)
            .await?;
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
            insert_timeline_events_with_tx(&tx, timeline_events).await?;
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
            insert_timeline_events_with_tx(&tx, timeline_events).await?;
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
        use entities::{agent_runtime_event, agent_runtime_snapshot};

        let exists = agent_runtime_event::Entity::find()
            .filter(agent_runtime_event::Column::SessionId.eq(session_id.to_string()))
            .filter(agent_runtime_event::Column::InferenceId.eq(delta.inference_id.clone()))
            .one(&self.db)
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
        .insert(&self.db)
        .await?;

        let existing = agent_runtime_snapshot::Entity::find()
            .filter(agent_runtime_snapshot::Column::SessionId.eq(session_id.to_string()))
            .filter(agent_runtime_snapshot::Column::AgentId.eq(delta.agent_id.clone()))
            .one(&self.db)
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
            active.update(&self.db).await?;
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
            .insert(&self.db)
            .await?;
        }

        self.rebuild_session_runtime_from_agent_snapshots(session_id)
            .await?;
        Ok(true)
    }

    pub async fn rebuild_session_runtime_from_agent_snapshots(
        &self,
        session_id: &str,
    ) -> Result<()> {
        use entities::{agent_runtime_snapshot, session_runtime_snapshot};

        let rows = agent_runtime_snapshot::Entity::find()
            .filter(agent_runtime_snapshot::Column::SessionId.eq(session_id.to_string()))
            .all(&self.db)
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
            .one(&self.db)
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
            active.update(&self.db).await?;
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
            .insert(&self.db)
            .await?;
        }
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

async fn insert_timeline_events_with_tx(
    tx: &sea_orm::DatabaseTransaction,
    timeline_events: &[TraceEvent],
) -> Result<()> {
    if timeline_events.is_empty() {
        return Ok(());
    }
    use entities::timeline_event;
    let models: Vec<timeline_event::ActiveModel> = timeline_events
        .iter()
        .map(|event| {
            let payload = serde_json::to_string(&event.kind).unwrap_or_default();
            timeline_event::ActiveModel {
                id: Set(new_timeline_event_id()),
                session_id: Set(event.session_id.clone()),
                sequence: Set(event.sequence as i64),
                created_at: Set(event.timestamp),
                kind: Set(trace_event_kind_label(&event.kind).to_string()),
                payload_json: Set(payload),
            }
        })
        .collect();
    timeline_event::Entity::insert_many(models).exec(tx).await?;
    Ok(())
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
