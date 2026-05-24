use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use pl_core::{
    ConfigStore, ModelConfig, ProjectRecord, ProviderConfig, ProviderEdit, ProviderModelEdit,
    ProviderSettingsEdit, ProviderTemplateKind, PureConfig, SessionRecord, StudioRuntime,
    ToolApprovalCallback, ToolApprovalDecision, ToolApprovalRequest, infer_provider_template_kind,
};
use pl_protocol::{AgentEvent, Message, MessageContent, MessageRole, PureError};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use tokio::sync::{Mutex, oneshot};

type ApprovalWaiters = Arc<Mutex<HashMap<String, oneshot::Sender<ToolApprovalDecision>>>>;
type CommandResult<T> = std::result::Result<T, CommandError>;

#[derive(Clone)]
struct AppState {
    studio: StudioRuntime,
    approvals: ApprovalWaiters,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CommandError {
    message: String,
}

impl CommandError {
    fn from_display(error: impl std::fmt::Display) -> Self {
        Self {
            message: error.to_string(),
        }
    }
}

impl From<anyhow::Error> for CommandError {
    fn from(error: anyhow::Error) -> Self {
        Self::from_display(error)
    }
}

impl From<PureError> for CommandError {
    fn from(error: PureError) -> Self {
        Self::from_display(error)
    }
}

impl From<std::io::Error> for CommandError {
    fn from(error: std::io::Error) -> Self {
        Self::from_display(error)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectDto {
    id: String,
    name: String,
    path: String,
    updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionDto {
    id: String,
    project_id: String,
    title: String,
    mode: String,
    updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct MessageDto {
    role: String,
    content: String,
    reasoning_content: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderDto {
    id: String,
    template_kind: String,
    name: String,
    subtitle: String,
    status: String,
    base_url: String,
    env_key: String,
    bearer_token: String,
    default_model: String,
    model_count: String,
    updated_at: String,
    wire_api: String,
    models: Vec<ModelDto>,
    default_models: Vec<ModelDto>,
    custom_models: Vec<ModelDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderTemplateDto {
    id: String,
    name: String,
    base_url: String,
    env_key: String,
    default_model: String,
    wire_api: String,
    default_models: Vec<ModelDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelDto {
    slug: String,
    display_name: String,
    reasoning_efforts: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConfigDto {
    toml: String,
    providers: Vec<ProviderDto>,
    templates: Vec<ProviderTemplateDto>,
    config_exists: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderSettingsInput {
    default_provider_id: Option<String>,
    providers: Vec<ProviderInput>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderInput {
    id: String,
    template_kind: String,
    name: String,
    base_url: String,
    env_key: String,
    bearer_token: String,
    default_model: String,
    wire_api: String,
    custom_models: Vec<ModelInput>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelInput {
    slug: String,
    display_name: String,
    reasoning_efforts: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BootstrapDto {
    projects: Vec<ProjectDto>,
    selected_project_id: Option<String>,
    sessions: Vec<SessionDto>,
    selected_session_id: Option<String>,
    messages: Vec<MessageDto>,
    config: ConfigDto,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectSelectionDto {
    project_id: String,
    projects: Vec<ProjectDto>,
    sessions: Vec<SessionDto>,
    selected_session_id: Option<String>,
    messages: Vec<MessageDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionSelectionDto {
    session_id: String,
    sessions: Vec<SessionDto>,
    messages: Vec<MessageDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunPromptResponse {
    session_id: String,
    messages: Vec<MessageDto>,
    sessions: Vec<SessionDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentEventPayload {
    session_id: String,
    event: AgentEvent,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolApprovalRequestPayload {
    approval_id: String,
    session_id: String,
    name: String,
    arguments: serde_json::Value,
    working_directory: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolApprovalResolvedPayload {
    approval_id: String,
    decision: String,
    reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PromptFailedPayload {
    session_id: Option<String>,
    message: String,
}

fn main() {
    let studio = tauri::async_runtime::block_on(StudioRuntime::default_app())
        .expect("failed to initialize Pure Studio runtime");
    let state = AppState {
        studio,
        approvals: Arc::new(Mutex::new(HashMap::new())),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            bootstrap_studio,
            open_project,
            select_project,
            create_session,
            select_session,
            run_prompt,
            approve_tool,
            deny_tool,
            load_config,
            save_config,
            save_provider_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Pure Studio");
}

#[tauri::command]
async fn bootstrap_studio(state: State<'_, AppState>) -> CommandResult<BootstrapDto> {
    let mut projects = state.studio.list_projects().await?;
    if projects.is_empty()
        && let Ok(cwd) = std::env::current_dir()
    {
        projects.push(state.studio.open_project(cwd).await?);
    }

    let mut selected_project_id = None;
    let mut sessions = Vec::new();
    let mut selected_session_id = None;
    let mut messages = Vec::new();

    if let Some(project) = projects.first() {
        selected_project_id = Some(project.id.clone());
        sessions = state.studio.ensure_project_sessions(&project.id).await?;
        if let Some(session) = sessions.first() {
            selected_session_id = Some(session.id.clone());
            messages = state.studio.store().load_messages(&session.id).await?;
        }
    }

    Ok(BootstrapDto {
        projects: project_dtos(projects),
        selected_project_id,
        sessions: session_dtos(sessions),
        selected_session_id,
        messages: message_dtos(messages),
        config: config_dto(state.studio.config_store())?,
    })
}

#[tauri::command]
async fn open_project(
    path: String,
    state: State<'_, AppState>,
) -> CommandResult<ProjectSelectionDto> {
    let path = PathBuf::from(path);
    if !path.is_dir() {
        return Err(CommandError::from_display(format!(
            "not a directory: {}",
            path.display()
        )));
    }
    let project = state.studio.open_project(path).await?;
    select_project_data(&state, project.id).await
}

#[tauri::command]
async fn select_project(
    project_id: String,
    state: State<'_, AppState>,
) -> CommandResult<ProjectSelectionDto> {
    select_project_data(&state, project_id).await
}

#[tauri::command]
async fn create_session(
    project_id: String,
    title: Option<String>,
    state: State<'_, AppState>,
) -> CommandResult<SessionSelectionDto> {
    let title = title
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "新会话".to_string());
    let session = state.studio.create_session(&project_id, &title).await?;
    let sessions = state.studio.store().list_sessions(&project_id).await?;
    Ok(SessionSelectionDto {
        session_id: session.id,
        sessions: session_dtos(sessions),
        messages: Vec::new(),
    })
}

#[tauri::command]
async fn select_session(
    session_id: String,
    state: State<'_, AppState>,
) -> CommandResult<SessionSelectionDto> {
    let messages = state.studio.store().load_messages(&session_id).await?;
    Ok(SessionSelectionDto {
        session_id,
        sessions: Vec::new(),
        messages: message_dtos(messages),
    })
}

#[tauri::command]
async fn run_prompt(
    session_id: String,
    prompt: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<RunPromptResponse> {
    if prompt.trim().is_empty() {
        return Err(CommandError::from_display("prompt is empty"));
    }

    let (event_tx, event_rx) = tokio::sync::broadcast::channel(256);
    let event_task =
        tauri::async_runtime::spawn(drain_events(session_id.clone(), event_rx, app.clone()));
    let result = state
        .studio
        .run_prompt(
            &session_id,
            prompt,
            event_tx.clone(),
            approval_callback(state.approvals.clone(), app.clone(), session_id.clone()),
        )
        .await;
    drop(event_tx);
    let _ = event_task.await;

    match result {
        Ok(outcome) => {
            let session = state
                .studio
                .store()
                .read_session(&session_id)
                .await?
                .context("selected session not found")?;
            let sessions = state
                .studio
                .store()
                .list_sessions(&session.project_id)
                .await?;
            let response = RunPromptResponse {
                session_id: session_id.clone(),
                messages: message_dtos(outcome.messages),
                sessions: session_dtos(sessions),
            };
            let _ = app.emit("studio-prompt-finished", response.clone());
            Ok(response)
        }
        Err(error) => {
            let message = error.to_string();
            let _ = app.emit(
                "studio-prompt-failed",
                PromptFailedPayload {
                    session_id: Some(session_id),
                    message: message.clone(),
                },
            );
            Err(CommandError { message })
        }
    }
}

#[tauri::command]
async fn approve_tool(
    approval_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<()> {
    resolve_tool_approval(
        approval_id,
        ToolApprovalDecision::Approved,
        app,
        state.approvals.clone(),
    )
    .await;
    Ok(())
}

#[tauri::command]
async fn deny_tool(
    approval_id: String,
    reason: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<()> {
    let reason = reason
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "denied by user".to_string());
    resolve_tool_approval(
        approval_id,
        ToolApprovalDecision::Denied { reason },
        app,
        state.approvals.clone(),
    )
    .await;
    Ok(())
}

#[tauri::command]
fn load_config(state: State<'_, AppState>) -> CommandResult<ConfigDto> {
    config_dto(state.studio.config_store())
}

#[tauri::command]
fn save_config(toml: String, state: State<'_, AppState>) -> CommandResult<ConfigDto> {
    let config = PureConfig::from_toml(&toml)?;
    state.studio.config_store().save(&config)?;
    config_dto(state.studio.config_store())
}

#[tauri::command]
fn save_provider_settings(
    input: ProviderSettingsInput,
    state: State<'_, AppState>,
) -> CommandResult<ConfigDto> {
    let current = state.studio.config_store().load_or_default()?;
    let edit = ProviderSettingsEdit {
        default_provider: input.default_provider_id,
        providers: input
            .providers
            .into_iter()
            .map(provider_edit)
            .collect::<CommandResult<Vec<_>>>()?,
    };
    let config = edit.to_config(&current)?;
    state.studio.config_store().save(&config)?;
    config_dto(state.studio.config_store())
}

async fn select_project_data(
    state: &State<'_, AppState>,
    project_id: String,
) -> CommandResult<ProjectSelectionDto> {
    state
        .studio
        .store()
        .mark_project_opened(&project_id)
        .await?;
    let sessions = state.studio.ensure_project_sessions(&project_id).await?;
    let selected_session_id = sessions.first().map(|session| session.id.clone());
    let messages = match &selected_session_id {
        Some(session_id) => state.studio.store().load_messages(session_id).await?,
        None => Vec::new(),
    };
    Ok(ProjectSelectionDto {
        project_id,
        projects: project_dtos(state.studio.list_projects().await?),
        sessions: session_dtos(sessions),
        selected_session_id,
        messages: message_dtos(messages),
    })
}

fn approval_callback(
    approvals: ApprovalWaiters,
    app: AppHandle,
    session_id: String,
) -> ToolApprovalCallback {
    Arc::new(move |request: ToolApprovalRequest| {
        let approvals = approvals.clone();
        let app = app.clone();
        let session_id = session_id.clone();
        Box::pin(async move {
            let (tx, rx) = oneshot::channel();
            approvals.lock().await.insert(request.id.clone(), tx);
            let _ = app.emit(
                "studio-tool-approval-requested",
                ToolApprovalRequestPayload {
                    approval_id: request.id,
                    session_id,
                    name: request.name,
                    arguments: request.arguments,
                    working_directory: request.working_directory,
                },
            );

            rx.await.unwrap_or_else(|_| ToolApprovalDecision::Denied {
                reason: "approval channel closed".to_string(),
            })
        })
    })
}

async fn resolve_tool_approval(
    approval_id: String,
    decision: ToolApprovalDecision,
    app: AppHandle,
    approvals: ApprovalWaiters,
) {
    if let Some(sender) = approvals.lock().await.remove(&approval_id) {
        let _ = sender.send(decision.clone());
    }
    let (decision_label, reason) = match decision {
        ToolApprovalDecision::Approved => ("approved".to_string(), None),
        ToolApprovalDecision::Denied { reason } => ("denied".to_string(), Some(reason)),
    };
    let _ = app.emit(
        "studio-tool-approval-resolved",
        ToolApprovalResolvedPayload {
            approval_id,
            decision: decision_label,
            reason,
        },
    );
}

async fn drain_events(
    session_id: String,
    mut event_rx: tokio::sync::broadcast::Receiver<AgentEvent>,
    app: AppHandle,
) {
    loop {
        let Ok(event) = event_rx.recv().await else {
            break;
        };
        let done = matches!(event, AgentEvent::Done);
        let _ = app.emit(
            "studio-agent-event",
            AgentEventPayload {
                session_id: session_id.clone(),
                event,
            },
        );
        if done {
            break;
        }
    }
}

fn config_dto(store: &ConfigStore) -> CommandResult<ConfigDto> {
    let config = store.load_or_default()?;
    Ok(ConfigDto {
        toml: config.to_toml_pretty()?,
        providers: provider_dtos(&config),
        templates: provider_template_dtos()?,
        config_exists: store.config_exists(),
    })
}

fn provider_dtos(config: &PureConfig) -> Vec<ProviderDto> {
    config
        .providers
        .iter()
        .map(|(provider_key, provider)| provider_dto(provider_key, provider))
        .collect()
}

fn provider_dto(provider_key: &str, provider: &ProviderConfig) -> ProviderDto {
    let kind = infer_provider_template_kind(provider_key, provider);
    let default_slugs = kind.default_model_slugs();
    let default_models = kind.default_models().unwrap_or_default();
    let custom_models = provider
        .models
        .iter()
        .filter(|model| !default_slugs.contains(&model.slug.as_str()))
        .map(model_dto)
        .collect::<Vec<_>>();
    let models = default_models
        .iter()
        .map(model_dto)
        .chain(custom_models.iter().cloned())
        .collect::<Vec<_>>();
    ProviderDto {
        id: provider_key.to_string(),
        template_kind: kind.key().to_string(),
        name: provider.name.clone(),
        subtitle: format!("{} Platform", provider.name),
        status: provider_status(provider).to_string(),
        base_url: provider.base_url.clone().unwrap_or_default(),
        env_key: provider.env_key.clone().unwrap_or_default(),
        bearer_token: provider.bearer_token.clone().unwrap_or_default(),
        default_model: provider.default_model.clone(),
        model_count: models.len().to_string(),
        updated_at: "Loaded".to_string(),
        wire_api: provider.wire_api.to_string(),
        models,
        default_models: default_models.iter().map(model_dto).collect(),
        custom_models,
    }
}

fn provider_template_dtos() -> CommandResult<Vec<ProviderTemplateDto>> {
    ProviderTemplateKind::all()
        .into_iter()
        .map(provider_template_dto)
        .collect()
}

fn provider_template_dto(kind: ProviderTemplateKind) -> CommandResult<ProviderTemplateDto> {
    let info = kind.provider_config()?;
    Ok(ProviderTemplateDto {
        id: kind.key().to_string(),
        name: info.name,
        base_url: info.base_url.unwrap_or_default(),
        env_key: info.env_key.unwrap_or_default(),
        default_model: info.default_model,
        wire_api: info.wire_api.to_string(),
        default_models: kind.default_models()?.iter().map(model_dto).collect(),
    })
}

fn provider_status(provider: &ProviderConfig) -> &'static str {
    if provider.bearer_token.is_some()
        || provider.env_key.is_some()
        || provider.auth_command.is_some()
    {
        "Healthy"
    } else {
        "Needs setup"
    }
}

fn model_dto(model: &ModelConfig) -> ModelDto {
    ModelDto {
        slug: model.slug.clone(),
        display_name: model.display_name.clone(),
        reasoning_efforts: model.reasoning_efforts.clone(),
    }
}

fn provider_edit(input: ProviderInput) -> CommandResult<ProviderEdit> {
    Ok(ProviderEdit {
        key: input.id,
        kind: provider_template_kind(&input.template_kind)?,
        name: input.name,
        base_url: Some(input.base_url),
        env_key: Some(input.env_key),
        bearer_token: Some(input.bearer_token),
        default_model: input.default_model,
        wire_api: input.wire_api,
        custom_models: input
            .custom_models
            .into_iter()
            .map(|model| ProviderModelEdit {
                slug: model.slug,
                display_name: model.display_name,
                reasoning_efforts: model.reasoning_efforts,
            })
            .collect(),
    })
}

fn provider_template_kind(value: &str) -> CommandResult<ProviderTemplateKind> {
    ProviderTemplateKind::from_key(value).ok_or_else(|| {
        CommandError::from_display(format!("unsupported provider template: {value}"))
    })
}

fn project_dtos(projects: Vec<ProjectRecord>) -> Vec<ProjectDto> {
    projects
        .into_iter()
        .map(|project| ProjectDto {
            id: project.id,
            name: project.name,
            path: project.path,
            updated_at: project.updated_at,
        })
        .collect()
}

fn session_dtos(sessions: Vec<SessionRecord>) -> Vec<SessionDto> {
    sessions
        .into_iter()
        .map(|session| SessionDto {
            id: session.id,
            project_id: session.project_id,
            title: session.title,
            mode: session.mode,
            updated_at: session.updated_at,
        })
        .collect()
}

fn message_dtos(messages: Vec<Message>) -> Vec<MessageDto> {
    messages.into_iter().map(message_dto).collect()
}

fn message_dto(message: Message) -> MessageDto {
    let role = match message.role {
        MessageRole::System => "system",
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::Tool => "tool",
    }
    .to_string();
    MessageDto {
        role,
        content: message_content_text(message.content),
        reasoning_content: message.reasoning_content,
    }
}

fn message_content_text(content: MessageContent) -> String {
    match content {
        MessageContent::Text(text) => text,
        MessageContent::MultiPart(parts) => {
            serde_json::to_string_pretty(&parts).unwrap_or_default()
        }
    }
}
