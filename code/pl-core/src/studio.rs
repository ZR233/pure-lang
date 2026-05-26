use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use pl_protocol::{AgentEventSender, Message, MessageContent, MessageRole};
use sea_orm::entity::prelude::*;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ConnectOptions, ConnectionTrait, Database, DatabaseBackend,
    DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, Statement,
};

const STUDIO_DIR_NAME: &str = "studio";
const STUDIO_DB_FILE_NAME: &str = "studio_1.sqlite";
use crate::config::{CONFIG_DIR_NAME, ConfigStore, ModelRole};
use crate::{
    CompileMode, CoreSession, PureCore, ToolApprovalCallback, TurnOptions, TurnRequest, TurnResult,
    load_workspace_instructions,
};
const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_init.sql"),
    include_str!("../migrations/0002_subagent_events.sql"),
    include_str!("../migrations/0003_session_runtime.sql"),
];

static ID_COUNTER: AtomicU64 = AtomicU64::new(0);

pub mod entities {
    use sea_orm::entity::prelude::*;

    pub mod project {
        use super::*;

        #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
        #[sea_orm(table_name = "projects")]
        pub struct Model {
            #[sea_orm(primary_key, auto_increment = false)]
            pub id: String,
            pub name: String,
            pub path: String,
            pub created_at: i64,
            pub updated_at: i64,
            pub last_opened_at: Option<i64>,
        }

        #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
        pub enum Relation {}

        impl ActiveModelBehavior for ActiveModel {}
    }

    pub mod session {
        use super::*;

        #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
        #[sea_orm(table_name = "sessions")]
        pub struct Model {
            #[sea_orm(primary_key, auto_increment = false)]
            pub id: String,
            pub project_id: String,
            pub title: String,
            pub mode: String,
            pub created_at: i64,
            pub updated_at: i64,
            pub archived: i32,
        }

        #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
        pub enum Relation {}

        impl ActiveModelBehavior for ActiveModel {}
    }

    pub mod message {
        use super::*;

        #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
        #[sea_orm(table_name = "messages")]
        pub struct Model {
            #[sea_orm(primary_key, auto_increment = false)]
            pub id: String,
            pub session_id: String,
            pub role: String,
            pub content: String,
            pub reasoning_content: Option<String>,
            pub metadata_json: String,
            pub created_at: i64,
        }

        #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
        pub enum Relation {}

        impl ActiveModelBehavior for ActiveModel {}
    }

    pub mod tool_approval {
        use super::*;

        #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
        #[sea_orm(table_name = "tool_approvals")]
        pub struct Model {
            #[sea_orm(primary_key, auto_increment = false)]
            pub id: String,
            pub session_id: String,
            pub tool_call_id: String,
            pub tool_name: String,
            pub arguments_json: String,
            pub working_directory: Option<String>,
            pub decision: String,
            pub reason: Option<String>,
            pub created_at: i64,
        }

        #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
        pub enum Relation {}

        impl ActiveModelBehavior for ActiveModel {}
    }

    pub mod subagent_event {
        use super::*;

        #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
        #[sea_orm(table_name = "subagent_events")]
        pub struct Model {
            #[sea_orm(primary_key, auto_increment = false)]
            pub id: String,
            pub session_id: String,
            pub subagent_id: String,
            pub parent_id: Option<String>,
            pub role: String,
            pub task: String,
            pub status: String,
            pub summary: Option<String>,
            pub depth: i32,
            pub error: Option<String>,
            pub created_at: i64,
        }

        #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
        pub enum Relation {}

        impl ActiveModelBehavior for ActiveModel {}
    }

    pub mod session_runtime_snapshot {
        use super::*;

        #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
        #[sea_orm(table_name = "session_runtime_snapshots")]
        pub struct Model {
            #[sea_orm(primary_key, auto_increment = false)]
            pub session_id: String,
            pub model: String,
            pub context_window: Option<i64>,
            pub latest_context_tokens: i64,
            pub prompt_tokens: i64,
            pub completion_tokens: i64,
            pub cached_prompt_tokens: i64,
            pub total_tokens: i64,
            pub currency: Option<String>,
            pub estimated_cost: Option<f64>,
            pub updated_at: i64,
        }

        #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
        pub enum Relation {}

        impl ActiveModelBehavior for ActiveModel {}
    }

    pub mod app_setting {
        use super::*;

        #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
        #[sea_orm(table_name = "app_settings")]
        pub struct Model {
            #[sea_orm(primary_key, auto_increment = false)]
            pub key: String,
            pub value: String,
            pub updated_at: i64,
        }

        #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
        pub enum Relation {}

        impl ActiveModelBehavior for ActiveModel {}
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRecord {
    pub id: String,
    pub name: String,
    pub path: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRecord {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub mode: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolApprovalRecord {
    pub session_id: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub arguments_json: String,
    pub working_directory: Option<String>,
    pub decision: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentEventRecord {
    pub event_id: String,
    pub session_id: String,
    pub subagent_id: String,
    pub parent_id: Option<String>,
    pub role: String,
    pub task: String,
    pub status: String,
    pub summary: Option<String>,
    pub depth: i32,
    pub error: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionRuntimeRecord {
    pub session_id: String,
    pub model: String,
    pub context_window: Option<u64>,
    pub latest_context_tokens: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cached_prompt_tokens: u64,
    pub total_tokens: u64,
    pub currency: Option<String>,
    pub estimated_cost: Option<f64>,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct StudioPromptOutcome {
    pub result: TurnResult,
    pub messages: Vec<Message>,
}

#[derive(Clone)]
pub struct StudioStore {
    db: DatabaseConnection,
}

#[derive(Clone)]
pub struct StudioRuntime {
    store: StudioStore,
    config_store: ConfigStore,
}

impl StudioRuntime {
    pub async fn default_app() -> Result<Self> {
        Ok(Self {
            store: StudioStore::default_app().await?,
            config_store: ConfigStore::default_app()?,
        })
    }

    pub fn new(store: StudioStore, config_store: ConfigStore) -> Self {
        Self {
            store,
            config_store,
        }
    }

    pub fn store(&self) -> &StudioStore {
        &self.store
    }

    pub fn config_store(&self) -> &ConfigStore {
        &self.config_store
    }

    pub async fn open_project(&self, path: impl AsRef<Path>) -> Result<ProjectRecord> {
        self.store.upsert_project(path).await
    }

    pub async fn list_projects(&self) -> Result<Vec<ProjectRecord>> {
        self.store.list_projects().await
    }

    pub async fn ensure_project_sessions(&self, project_id: &str) -> Result<Vec<SessionRecord>> {
        let mut sessions = self.store.list_sessions(project_id).await?;
        if sessions.is_empty() {
            sessions.push(
                self.store
                    .create_session(project_id, "新会话", CompileMode::Auto)
                    .await?,
            );
        }
        Ok(sessions)
    }

    pub async fn create_session(&self, project_id: &str, title: &str) -> Result<SessionRecord> {
        self.store
            .create_session(project_id, title, CompileMode::Auto)
            .await
    }

    pub async fn session_runtime(&self, session_id: &str) -> Result<SessionRuntimeRecord> {
        if let Some(snapshot) = self.store.load_session_runtime(session_id).await? {
            return Ok(snapshot);
        }
        let config = self.config_store.load_or_default()?;
        let resolved = config.resolve_role(ModelRole::Planner)?;
        let model = resolved
            .models
            .iter()
            .find(|model| model.slug == resolved.role_config.model)
            .or_else(|| resolved.models.first());
        Ok(default_session_runtime_record(session_id, model))
    }

    pub async fn run_prompt(
        &self,
        session_id: &str,
        prompt: String,
        event_tx: AgentEventSender,
        _approval_callback: ToolApprovalCallback,
    ) -> Result<StudioPromptOutcome> {
        let session_record = self
            .store
            .read_session(session_id)
            .await?
            .context("selected session not found")?;
        let project = self
            .store
            .read_project(&session_record.project_id)
            .await?
            .context("selected project not found")?;
        let mut session = self.store.load_core_session(session_id).await?;
        let previous_len = session.len();
        let config = self.config_store.load_or_default()?;
        let workspace_instructions = load_workspace_instructions(Path::new(&project.path))?;
        let mut request = TurnRequest::new(prompt.clone(), CompileMode::Auto);
        if !workspace_instructions.trim().is_empty() {
            request = request.with_workspace_instructions(workspace_instructions.clone());
        }

        let mut core = PureCore::from_config(&config, ModelRole::Planner)?;
        core.register_default_tools(PathBuf::from(&project.path), Some(workspace_instructions));
        let options = TurnOptions::default();
        let result = core
            .run_turn_with_options(&mut session, request, event_tx, options)
            .await?;
        let resolved = config.resolve_role(ModelRole::Planner)?;
        let model = resolved
            .models
            .iter()
            .find(|model| model.slug == result.model)
            .or_else(|| {
                resolved
                    .models
                    .iter()
                    .find(|model| model.slug == resolved.role_config.model)
            })
            .or_else(|| resolved.models.first());
        self.store
            .upsert_session_runtime(session_id, &result, model)
            .await?;
        self.store
            .append_messages(session_id, &session.messages()[previous_len..])
            .await?;
        if previous_len == 0 {
            self.store
                .rename_session(session_id, &session_title_from_prompt(&prompt))
                .await?;
        }
        let messages = self.store.load_messages(session_id).await?;
        Ok(StudioPromptOutcome { result, messages })
    }
}

impl StudioStore {
    pub async fn default_app() -> Result<Self> {
        let db_path = default_db_path()?;
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
        for message in messages {
            self.append_message(session_id, message).await?;
        }
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

    pub async fn record_subagent_event(&self, record: SubagentEventRecord) -> Result<()> {
        use entities::subagent_event;
        subagent_event::ActiveModel {
            id: Set(record.event_id),
            session_id: Set(record.session_id),
            subagent_id: Set(record.subagent_id),
            parent_id: Set(record.parent_id),
            role: Set(record.role),
            task: Set(record.task),
            status: Set(record.status),
            summary: Set(record.summary),
            depth: Set(record.depth),
            error: Set(record.error),
            created_at: Set(record.created_at),
        }
        .insert(&self.db)
        .await?;
        Ok(())
    }

    pub async fn list_subagent_events(&self, session_id: &str) -> Result<Vec<SubagentEventRecord>> {
        use entities::subagent_event;
        let rows = subagent_event::Entity::find()
            .filter(subagent_event::Column::SessionId.eq(session_id.to_string()))
            .order_by_asc(subagent_event::Column::CreatedAt)
            .order_by_asc(subagent_event::Column::Id)
            .all(&self.db)
            .await?;
        Ok(rows.into_iter().map(subagent_event_record).collect())
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

fn project_record(model: entities::project::Model) -> ProjectRecord {
    ProjectRecord {
        id: model.id,
        name: model.name,
        path: model.path,
        updated_at: model.updated_at,
    }
}

fn session_record(model: entities::session::Model) -> SessionRecord {
    SessionRecord {
        id: model.id,
        project_id: model.project_id,
        title: model.title,
        mode: model.mode,
        updated_at: model.updated_at,
    }
}

fn subagent_event_record(model: entities::subagent_event::Model) -> SubagentEventRecord {
    SubagentEventRecord {
        event_id: model.id,
        session_id: model.session_id,
        subagent_id: model.subagent_id,
        parent_id: model.parent_id,
        role: model.role,
        task: model.task,
        status: model.status,
        summary: model.summary,
        depth: model.depth,
        error: model.error,
        created_at: model.created_at,
    }
}

fn session_runtime_record(
    model: entities::session_runtime_snapshot::Model,
) -> SessionRuntimeRecord {
    SessionRuntimeRecord {
        session_id: model.session_id,
        model: model.model,
        context_window: model.context_window.map(|value| value as u64),
        latest_context_tokens: model.latest_context_tokens as u64,
        prompt_tokens: model.prompt_tokens as u64,
        completion_tokens: model.completion_tokens as u64,
        cached_prompt_tokens: model.cached_prompt_tokens as u64,
        total_tokens: model.total_tokens as u64,
        currency: model.currency,
        estimated_cost: model.estimated_cost,
        updated_at: model.updated_at,
    }
}

fn default_session_runtime_record(
    session_id: &str,
    model: Option<&pl_model::ModelInfo>,
) -> SessionRuntimeRecord {
    SessionRuntimeRecord {
        session_id: session_id.to_string(),
        model: model
            .map(|model| model.slug.clone())
            .unwrap_or_else(|| "unknown".to_string()),
        context_window: model.and_then(pl_model::ModelInfo::resolved_context_window),
        latest_context_tokens: 0,
        prompt_tokens: 0,
        completion_tokens: 0,
        cached_prompt_tokens: 0,
        total_tokens: 0,
        currency: model.and_then(|model| model.currency.clone()),
        estimated_cost: None,
        updated_at: unix_seconds(),
    }
}

fn estimate_cost(
    prompt_tokens: u64,
    completion_tokens: u64,
    cached_prompt_tokens: u64,
    input_price_per_mtok: Option<f64>,
    output_price_per_mtok: Option<f64>,
    cache_read_price_per_mtok: Option<f64>,
) -> Option<f64> {
    let input_price = input_price_per_mtok?;
    let output_price = output_price_per_mtok?;
    let cache_price = cache_read_price_per_mtok?;
    let cached = cached_prompt_tokens.min(prompt_tokens);
    let uncached_input = prompt_tokens.saturating_sub(cached);
    Some(
        (uncached_input as f64 * input_price
            + completion_tokens as f64 * output_price
            + cached as f64 * cache_price)
            / 1_000_000.0,
    )
}

fn row_to_message(row: entities::message::Model) -> Result<Message> {
    let role = match row.role.as_str() {
        "system" => MessageRole::System,
        "user" => MessageRole::User,
        "assistant" => MessageRole::Assistant,
        "tool" => MessageRole::Tool,
        other => bail!("unsupported message role in studio db: {other}"),
    };
    let metadata = serde_json::from_str(&row.metadata_json)
        .with_context(|| format!("failed to parse message metadata: {}", row.id))?;
    Ok(Message {
        role,
        content: MessageContent::Text(row.content),
        reasoning_content: row.reasoning_content,
        metadata,
    })
}

fn message_to_row_parts(message: &Message) -> Result<(String, String)> {
    let role = match message.role {
        MessageRole::System => "system",
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::Tool => "tool",
    };
    let content = match &message.content {
        MessageContent::Text(text) => text.clone(),
        MessageContent::MultiPart(parts) => serde_json::to_string(parts)?,
    };
    Ok((role.to_string(), content))
}

async fn configure_sqlite(db: &DatabaseConnection) -> Result<()> {
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

async fn run_migrations(db: &DatabaseConnection) -> Result<()> {
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

fn split_sql(sql: &str) -> Vec<String> {
    sql.split(';')
        .map(str::trim)
        .filter(|statement| !statement.is_empty())
        .map(|statement| format!("{statement};"))
        .collect()
}

pub fn default_db_path() -> Result<PathBuf> {
    Ok(user_home_dir()?
        .join(CONFIG_DIR_NAME)
        .join(STUDIO_DIR_NAME)
        .join(STUDIO_DB_FILE_NAME))
}

fn user_home_dir() -> Result<PathBuf> {
    #[cfg(windows)]
    const HOME_VARS: &[&str] = &["USERPROFILE", "HOME"];
    #[cfg(not(windows))]
    const HOME_VARS: &[&str] = &["HOME", "USERPROFILE"];

    HOME_VARS
        .iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .find(|path| !path.as_os_str().is_empty())
        .context("could not resolve user home directory")
}

fn sqlite_url(path: &Path) -> String {
    let path = path.to_string_lossy().replace('\\', "/");
    format!("sqlite://{path}?mode=rwc")
}

fn project_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| path.display().to_string())
}

fn non_empty_title(title: &str) -> String {
    let title = title.trim();
    if title.is_empty() {
        "新会话".to_string()
    } else {
        title.chars().take(80).collect()
    }
}

fn new_id(prefix: &str) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros();
    let seq = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{now:x}-{seq:x}")
}

fn unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn session_title_from_prompt(prompt: &str) -> String {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return "新会话".to_string();
    }
    prompt.chars().take(42).collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use pretty_assertions::assert_eq;

    use super::*;

    #[tokio::test]
    async fn project_crud_orders_by_recent_open() {
        let store = StudioStore::open_memory().await.unwrap();

        let first = store.upsert_project("C:/work/alpha").await.unwrap();
        let second = store.upsert_project("C:/work/beta").await.unwrap();
        tokio::time::sleep(Duration::from_secs(1)).await;
        store.mark_project_opened(&first.id).await.unwrap();

        let projects = store.list_projects().await.unwrap();

        assert_eq!(projects.len(), 2);
        assert_eq!(projects[0].id, first.id);
        assert_eq!(projects[1].id, second.id);
    }

    #[tokio::test]
    async fn session_crud_and_message_restore() {
        let store = StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project("C:/work/alpha").await.unwrap();
        let session = store
            .create_session(&project.id, "Build app", CompileMode::Auto)
            .await
            .unwrap();
        let message = Message {
            role: MessageRole::User,
            content: MessageContent::Text("hello".to_string()),
            reasoning_content: None,
            metadata: HashMap::new(),
        };

        store.append_message(&session.id, &message).await.unwrap();
        let restored = store.load_core_session(&session.id).await.unwrap();

        assert_eq!(restored.len(), 1);
        assert_eq!(restored.messages()[0].role, MessageRole::User);
        match &restored.messages()[0].content {
            MessageContent::Text(text) => assert_eq!(text, "hello"),
            MessageContent::MultiPart(_) => panic!("expected text message"),
        }
    }

    #[tokio::test]
    async fn records_tool_approval() {
        let store = StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project("C:/work/alpha").await.unwrap();
        let session = store
            .create_session(&project.id, "Build app", CompileMode::Auto)
            .await
            .unwrap();

        store
            .record_tool_approval(ToolApprovalRecord {
                session_id: session.id,
                tool_call_id: "call-1".to_string(),
                tool_name: "bash".to_string(),
                arguments_json: "{}".to_string(),
                working_directory: None,
                decision: "approved".to_string(),
                reason: None,
            })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn records_subagent_events_in_session_order() {
        let store = StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project("C:/work/alpha").await.unwrap();
        let session = store
            .create_session(&project.id, "Build app", CompileMode::Auto)
            .await
            .unwrap();

        store
            .record_subagent_event(SubagentEventRecord {
                event_id: "event-1".to_string(),
                session_id: session.id.clone(),
                subagent_id: "subagent-1".to_string(),
                parent_id: None,
                role: "executor".to_string(),
                task: "inspect".to_string(),
                status: "running".to_string(),
                summary: None,
                depth: 1,
                error: None,
                created_at: 10,
            })
            .await
            .unwrap();
        store
            .record_subagent_event(SubagentEventRecord {
                event_id: "event-2".to_string(),
                session_id: session.id.clone(),
                subagent_id: "subagent-1".to_string(),
                parent_id: None,
                role: "executor".to_string(),
                task: "inspect".to_string(),
                status: "succeeded".to_string(),
                summary: Some("done".to_string()),
                depth: 1,
                error: None,
                created_at: 11,
            })
            .await
            .unwrap();

        let events = store.list_subagent_events(&session.id).await.unwrap();

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].status, "running");
        assert_eq!(events[1].summary.as_deref(), Some("done"));
    }

    #[tokio::test]
    async fn session_runtime_snapshot_accumulates_usage_and_cost() {
        let store = StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project("C:/work/alpha").await.unwrap();
        let session = store
            .create_session(&project.id, "Build app", CompileMode::Auto)
            .await
            .unwrap();
        let mut model = pl_model::ModelInfo::fallback("priced-model");
        model.context_window = Some(1_000_000);
        model.currency = Some("CNY".to_string());
        model.input_price_per_mtok = Some(8.0);
        model.output_price_per_mtok = Some(32.0);
        model.cache_read_price_per_mtok = Some(2.0);
        let result = TurnResult {
            content: "ok".to_string(),
            reasoning_content: None,
            model: "priced-model".to_string(),
            usage: pl_model::TokenUsage {
                prompt_tokens: 100_000,
                completion_tokens: 10_000,
                total_tokens: 110_000,
                cached_prompt_tokens: 40_000,
            },
            mode: CompileMode::Auto,
            session_message_count: 2,
        };

        store
            .upsert_session_runtime(&session.id, &result, Some(&model))
            .await
            .unwrap();
        store
            .upsert_session_runtime(&session.id, &result, Some(&model))
            .await
            .unwrap();

        let runtime = store
            .load_session_runtime(&session.id)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(runtime.model, "priced-model");
        assert_eq!(runtime.context_window, Some(1_000_000));
        assert_eq!(runtime.latest_context_tokens, 100_000);
        assert_eq!(runtime.prompt_tokens, 200_000);
        assert_eq!(runtime.completion_tokens, 20_000);
        assert_eq!(runtime.cached_prompt_tokens, 80_000);
        assert_eq!(runtime.currency.as_deref(), Some("CNY"));
        assert!(
            runtime
                .estimated_cost
                .is_some_and(|cost| (cost - 1.76).abs() < 0.000_001)
        );
    }

    #[tokio::test]
    async fn settings_round_trip() {
        let store = StudioStore::open_memory().await.unwrap();

        store
            .save_setting("activeProject", "project-1")
            .await
            .unwrap();
        let value = store.load_setting("activeProject").await.unwrap();

        assert_eq!(value.as_deref(), Some("project-1"));
    }
}
