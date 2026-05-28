use std::path::Path;

use anyhow::{Context, Result};
use pl_protocol::AgentEventSender;

use crate::config::{ConfigStore, ModelRole};
use crate::studio::StudioStore;
use crate::studio::mappers::default_session_runtime_record;
use crate::studio::records::{
    ProjectRecord, SessionRecord, SessionRuntimeRecord, StudioPromptOutcome,
};
use crate::{
    CompileMode, PureCore, ToolApprovalCallback, TraceRecorder, TurnOptions, TurnRequest,
    load_workspace_instructions, resolve_workspace_root,
};

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
        approval_callback: ToolApprovalCallback,
        mut options: TurnOptions,
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
        let workspace_root = resolve_workspace_root(Path::new(&project.path))?;
        let workspace_instructions = load_workspace_instructions(&workspace_root)?;
        let mut request = TurnRequest::new(prompt.clone(), CompileMode::Auto);
        if !workspace_instructions.trim().is_empty() {
            request = request.with_workspace_instructions(workspace_instructions.clone());
        }

        let mut core = PureCore::from_config(&config, ModelRole::Planner)?;
        core.register_default_tools(workspace_root, Some(workspace_instructions));
        if matches!(
            options.tool_approval_policy,
            crate::turn::ToolApprovalPolicy::Manual
        ) && options.tool_approval_callback.is_none()
        {
            options.tool_approval_callback = Some(approval_callback);
        }
        let starting_sequence = self.store.next_sequence(session_id).await?;
        let mut recorder = TraceRecorder::new(session_id.to_string(), event_tx, starting_sequence);
        let result = core
            .run_turn_with_trace(&mut session, request, &mut recorder, options)
            .await?;
        let trace_events = result.trace_events.clone();
        let new_messages = &session.messages()[previous_len..];
        self.store
            .append_turn_records(session_id, &trace_events, new_messages)
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
        if previous_len == 0 {
            self.store
                .rename_session(session_id, &session_title_from_prompt(&prompt))
                .await?;
        }
        let messages = self.store.load_messages(session_id).await?;
        Ok(StudioPromptOutcome {
            result,
            messages,
            trace_events,
        })
    }
}

fn session_title_from_prompt(prompt: &str) -> String {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return "新会话".to_string();
    }
    prompt.chars().take(42).collect()
}
