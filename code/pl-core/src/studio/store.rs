use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use pl_protocol::Message;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectOptions, Database, DatabaseConnection,
    EntityTrait, QueryFilter, QueryOrder, QuerySelect, TransactionTrait,
};

use crate::studio::entities;
use crate::studio::ids::{new_id, new_timeline_event_id, unix_seconds};
use crate::studio::mappers::{
    agent_snapshot_record, agent_timeline_event_record, estimate_cost, message_to_row_parts,
    project_record, row_to_message, session_record, session_runtime_record, timeline_event_record,
    trace_event_kind_label,
};
use crate::studio::paths::{prepare_database_switch, project_name, sqlite_url};
use crate::studio::records::{
    AgentSnapshotRecord, AgentTimelineEventRecord, ProjectRecord, SessionRecord,
    SessionRuntimeRecord, TimelineEventRecord, ToolApprovalRecord,
};
use crate::studio::store_support::{
    configure_sqlite, insert_message_with_tx, non_empty_title, run_migrations,
    touch_session_with_tx,
};
use crate::{CompileMode, CoreSession, TraceEvent, TurnResult};
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
        }
        .insert(&self.db)
        .await?;
        Ok(project_record(model))
    }

    pub async fn list_projects(&self) -> Result<Vec<ProjectRecord>> {
        use entities::project;
        let projects = project::Entity::find()
            .order_by_desc(project::Column::LastOpenedAt)
            .order_by_desc(project::Column::UpdatedAt)
            .order_by_desc(project::Column::Id)
            .all(&self.db)
            .await?;
        Ok(projects.into_iter().map(project_record).collect())
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
            active.update(&self.db).await?;
        }
        Ok(())
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

    pub async fn record_tool_approval(&self, record: ToolApprovalRecord) -> Result<()> {
        use entities::tool_approval;
        tool_approval::ActiveModel {
            id: Set(new_id("approval")),
            session_id: Set(record.session_id),
            tool_call_id: Set(record.tool_call_id),
            tool_name: Set(record.tool_name),
            arguments_json: Set(record.arguments_json),
            working_directory: Set(record.working_directory),
            decision: Set(record.decision),
            reason: Set(record.reason),
            created_at: Set(unix_seconds()),
        }
        .insert(&self.db)
        .await?;
        Ok(())
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
        Ok(rows.into_iter().map(agent_snapshot_record).collect())
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
            timeline_event::Entity::insert_many(models)
                .exec(&tx)
                .await?;
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
        use entities::session_runtime_snapshot;

        let existing = self.load_session_runtime(session_id).await?;
        let prompt_tokens = existing
            .as_ref()
            .map(|record| record.prompt_tokens)
            .unwrap_or(0)
            + result.usage.prompt_tokens;
        let completion_tokens = existing
            .as_ref()
            .map(|record| record.completion_tokens)
            .unwrap_or(0)
            + result.usage.completion_tokens;
        let cached_prompt_tokens = existing
            .as_ref()
            .map(|record| record.cached_prompt_tokens)
            .unwrap_or(0)
            + result.usage.cached_prompt_tokens;
        let total_tokens = prompt_tokens + completion_tokens;
        let model_name = if result.model.is_empty() {
            model
                .map(|model| model.slug.clone())
                .unwrap_or_else(|| "unknown".to_string())
        } else {
            result.model.clone()
        };
        let currency = model.and_then(|model| model.currency.clone());
        let estimated_cost = model.and_then(|model| {
            estimate_cost(
                prompt_tokens,
                completion_tokens,
                cached_prompt_tokens,
                model.input_price_per_mtok,
                model.output_price_per_mtok,
                model.cache_read_price_per_mtok,
            )
        });
        let now = unix_seconds();

        if let Some(row) = session_runtime_snapshot::Entity::find_by_id(session_id.to_string())
            .one(&self.db)
            .await?
        {
            let mut active: session_runtime_snapshot::ActiveModel = row.into();
            active.model = Set(model_name);
            active.context_window = Set(model
                .and_then(pl_model::ModelInfo::resolved_context_window)
                .map(|value| value as i64));
            active.latest_context_tokens = Set(result.usage.prompt_tokens as i64);
            active.prompt_tokens = Set(prompt_tokens as i64);
            active.completion_tokens = Set(completion_tokens as i64);
            active.cached_prompt_tokens = Set(cached_prompt_tokens as i64);
            active.total_tokens = Set(total_tokens as i64);
            active.currency = Set(currency);
            active.estimated_cost = Set(estimated_cost);
            active.updated_at = Set(now);
            active.update(&self.db).await?;
        } else {
            session_runtime_snapshot::ActiveModel {
                session_id: Set(session_id.to_string()),
                model: Set(model_name),
                context_window: Set(model
                    .and_then(pl_model::ModelInfo::resolved_context_window)
                    .map(|value| value as i64)),
                latest_context_tokens: Set(result.usage.prompt_tokens as i64),
                prompt_tokens: Set(prompt_tokens as i64),
                completion_tokens: Set(completion_tokens as i64),
                cached_prompt_tokens: Set(cached_prompt_tokens as i64),
                total_tokens: Set(total_tokens as i64),
                currency: Set(currency),
                estimated_cost: Set(estimated_cost),
                updated_at: Set(now),
            }
            .insert(&self.db)
            .await?;
        }
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
