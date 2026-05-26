use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use pl_core::{
    ConfigStore, ModelCapabilityConfig, ModelConfig, ModelRole, ProjectRecord, ProviderConfig,
    ProviderEdit, ProviderModelEdit, ProviderSettingsEdit, ProviderTemplateKind, PureConfig,
    RoleEdit, SessionRecord, SessionRuntimeRecord, StudioRuntime, StudioStore, SubagentEventRecord,
    ToolApprovalCallback, ToolApprovalDecision, ToolApprovalRequest, infer_provider_template_kind,
};
use pl_protocol::{AgentEvent, Message, MessageContent, MessageRole, PureError, SubagentStatus};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use tokio::sync::{Mutex, oneshot};

type ApprovalWaiters = Arc<Mutex<HashMap<String, oneshot::Sender<ToolApprovalDecision>>>>;
type CommandResult<T> = std::result::Result<T, CommandError>;

static EVENT_COUNTER: AtomicU64 = AtomicU64::new(0);

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
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SubagentEventDto {
    event_id: String,
    id: String,
    parent_id: Option<String>,
    role: String,
    task: String,
    status: String,
    summary: Option<String>,
    depth: i32,
    error: Option<String>,
    updated_at: i64,
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
    default_model: String,
    wire_api: String,
    default_models: Vec<ModelDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RoleDto {
    key: String,
    display_name: String,
    provider: String,
    model: String,
    effort: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelDto {
    slug: String,
    display_name: String,
    description: Option<String>,
    context_window: Option<u64>,
    max_context_window: Option<u64>,
    auto_compact_token_limit: Option<u64>,
    default_temperature: Option<f32>,
    max_output_tokens: Option<u64>,
    currency: Option<String>,
    #[serde(rename = "inputPricePerMTok")]
    input_price_per_mtok: Option<f64>,
    #[serde(rename = "outputPricePerMTok")]
    output_price_per_mtok: Option<f64>,
    #[serde(rename = "cacheReadPricePerMTok")]
    cache_read_price_per_mtok: Option<f64>,
    reasoning_efforts: Vec<String>,
    capabilities: Vec<String>,
    input_modalities: Vec<String>,
    truncation_mode: String,
    truncation_limit: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConfigDto {
    toml: String,
    providers: Vec<ProviderDto>,
    roles: Vec<RoleDto>,
    templates: Vec<ProviderTemplateDto>,
    config_exists: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionRuntimeDto {
    session_id: String,
    model: String,
    context_window: Option<u64>,
    latest_context_tokens: u64,
    prompt_tokens: u64,
    completion_tokens: u64,
    cached_prompt_tokens: u64,
    total_tokens: u64,
    cache_hit_rate: Option<f64>,
    currency: Option<String>,
    #[serde(rename = "inputPricePerMTok")]
    input_price_per_mtok: Option<f64>,
    #[serde(rename = "outputPricePerMTok")]
    output_price_per_mtok: Option<f64>,
    #[serde(rename = "cacheReadPricePerMTok")]
    cache_read_price_per_mtok: Option<f64>,
    estimated_cost: Option<f64>,
    active_skills: Vec<String>,
    active_mcp_servers: Vec<String>,
    updated_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderSettingsInput {
    default_provider_id: Option<String>,
    providers: Vec<ProviderInput>,
    #[serde(default)]
    roles: Vec<RoleInput>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderInput {
    id: String,
    template_kind: String,
    name: String,
    base_url: String,
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

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RoleInput {
    key: String,
    provider: String,
    model: String,
    effort: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BootstrapDto {
    projects: Vec<ProjectDto>,
    selected_project_id: Option<String>,
    sessions: Vec<SessionDto>,
    selected_session_id: Option<String>,
    messages: Vec<MessageDto>,
    subagent_events: Vec<SubagentEventDto>,
    session_runtime: Option<SessionRuntimeDto>,
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
    subagent_events: Vec<SubagentEventDto>,
    session_runtime: Option<SessionRuntimeDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionSelectionDto {
    session_id: String,
    sessions: Vec<SessionDto>,
    messages: Vec<MessageDto>,
    subagent_events: Vec<SubagentEventDto>,
    session_runtime: Option<SessionRuntimeDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunPromptResponse {
    session_id: String,
    messages: Vec<MessageDto>,
    sessions: Vec<SessionDto>,
    subagent_events: Vec<SubagentEventDto>,
    session_runtime: SessionRuntimeDto,
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
    parent_subagent_id: Option<String>,
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
    let mut subagent_events = Vec::new();

    if let Some(project) = projects.first() {
        selected_project_id = Some(project.id.clone());
        sessions = state.studio.ensure_project_sessions(&project.id).await?;
        if let Some(session) = sessions.first() {
            selected_session_id = Some(session.id.clone());
            messages = state.studio.store().load_messages(&session.id).await?;
            subagent_events = state
                .studio
                .store()
                .list_subagent_events(&session.id)
                .await?;
        }
    }
    let session_runtime = match selected_session_id.as_deref() {
        Some(session_id) => Some(load_session_runtime_dto(&state.studio, session_id).await?),
        None => None,
    };

    Ok(BootstrapDto {
        projects: project_dtos(projects),
        selected_project_id,
        sessions: session_dtos(sessions),
        selected_session_id,
        messages: message_dtos(messages),
        subagent_events: subagent_event_dtos(subagent_events),
        session_runtime,
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
        session_id: session.id.clone(),
        sessions: session_dtos(sessions),
        messages: Vec::new(),
        subagent_events: Vec::new(),
        session_runtime: Some(load_session_runtime_dto(&state.studio, &session.id).await?),
    })
}

#[tauri::command]
async fn select_session(
    session_id: String,
    state: State<'_, AppState>,
) -> CommandResult<SessionSelectionDto> {
    let messages = state.studio.store().load_messages(&session_id).await?;
    let subagent_events = state
        .studio
        .store()
        .list_subagent_events(&session_id)
        .await?;
    Ok(SessionSelectionDto {
        session_runtime: Some(load_session_runtime_dto(&state.studio, &session_id).await?),
        session_id,
        sessions: Vec::new(),
        messages: message_dtos(messages),
        subagent_events: subagent_event_dtos(subagent_events),
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
    let event_task = tauri::async_runtime::spawn(drain_events(
        session_id.clone(),
        event_rx,
        app.clone(),
        state.studio.store().clone(),
    ));
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
                subagent_events: subagent_event_dtos(
                    state
                        .studio
                        .store()
                        .list_subagent_events(&session_id)
                        .await?,
                ),
                session_runtime: load_session_runtime_dto(&state.studio, &session_id).await?,
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
        roles: input.roles.into_iter().map(role_edit).collect(),
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
    let subagent_events = match &selected_session_id {
        Some(session_id) => {
            state
                .studio
                .store()
                .list_subagent_events(session_id)
                .await?
        }
        None => Vec::new(),
    };
    let session_runtime = match selected_session_id.as_deref() {
        Some(session_id) => Some(load_session_runtime_dto(&state.studio, session_id).await?),
        None => None,
    };
    Ok(ProjectSelectionDto {
        project_id,
        projects: project_dtos(state.studio.list_projects().await?),
        sessions: session_dtos(sessions),
        selected_session_id,
        messages: message_dtos(messages),
        subagent_events: subagent_event_dtos(subagent_events),
        session_runtime,
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
                    parent_subagent_id: request.parent_subagent_id,
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
    store: StudioStore,
) {
    loop {
        let Ok(event) = event_rx.recv().await else {
            break;
        };
        let done = matches!(event, AgentEvent::Done);
        if let Some(record) = subagent_event_record(&session_id, &event) {
            let _ = store.record_subagent_event(record).await;
        }
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

fn subagent_event_record(session_id: &str, event: &AgentEvent) -> Option<SubagentEventRecord> {
    match event {
        AgentEvent::SubagentStateChanged {
            id,
            parent_id,
            role,
            task,
            status,
            summary,
            depth,
            error,
            updated_at,
        } => Some(SubagentEventRecord {
            event_id: new_event_id("subagent-event"),
            session_id: session_id.to_string(),
            subagent_id: id.clone(),
            parent_id: parent_id.clone(),
            role: role.clone(),
            task: task.clone(),
            status: subagent_status_label(*status).to_string(),
            summary: summary.clone(),
            depth: *depth as i32,
            error: error.clone(),
            created_at: *updated_at,
        }),
        _ => None,
    }
}

fn config_dto(store: &ConfigStore) -> CommandResult<ConfigDto> {
    let config = store.load_or_default()?;
    Ok(ConfigDto {
        toml: config.to_toml_pretty()?,
        providers: provider_dtos(&config),
        roles: role_dtos(&config),
        templates: provider_template_dtos()?,
        config_exists: store.config_exists(),
    })
}

async fn load_session_runtime_dto(
    studio: &StudioRuntime,
    session_id: &str,
) -> CommandResult<SessionRuntimeDto> {
    let config = studio.config_store().load_or_default()?;
    let record = studio.session_runtime(session_id).await?;
    Ok(session_runtime_dto(record, &config))
}

fn session_runtime_dto(record: SessionRuntimeRecord, config: &PureConfig) -> SessionRuntimeDto {
    let cache_hit_rate = if record.prompt_tokens == 0 {
        None
    } else {
        Some(record.cached_prompt_tokens as f64 / record.prompt_tokens as f64)
    };
    let current_model = config
        .resolve_role(ModelRole::Planner)
        .ok()
        .and_then(|resolved| {
            resolved
                .models
                .iter()
                .find(|model| model.slug == record.model)
                .cloned()
                .or_else(|| {
                    resolved
                        .models
                        .iter()
                        .find(|model| model.slug == resolved.role_config.model)
                        .cloned()
                })
        });
    SessionRuntimeDto {
        session_id: record.session_id,
        model: record.model,
        context_window: record.context_window,
        latest_context_tokens: record.latest_context_tokens,
        prompt_tokens: record.prompt_tokens,
        completion_tokens: record.completion_tokens,
        cached_prompt_tokens: record.cached_prompt_tokens,
        total_tokens: record.total_tokens,
        cache_hit_rate,
        currency: record.currency.or_else(|| {
            current_model
                .as_ref()
                .and_then(|model| model.currency.clone())
        }),
        input_price_per_mtok: current_model
            .as_ref()
            .and_then(|model| model.input_price_per_mtok),
        output_price_per_mtok: current_model
            .as_ref()
            .and_then(|model| model.output_price_per_mtok),
        cache_read_price_per_mtok: current_model
            .as_ref()
            .and_then(|model| model.cache_read_price_per_mtok),
        estimated_cost: record.estimated_cost,
        active_skills: config.runtime.active_skills.clone(),
        active_mcp_servers: config.runtime.active_mcp_servers.clone(),
        updated_at: record.updated_at,
    }
}

fn role_dtos(config: &PureConfig) -> Vec<RoleDto> {
    ModelRole::all()
        .into_iter()
        .map(|role| {
            let role_config = config.role_config(role);
            RoleDto {
                key: role.key().to_string(),
                display_name: role.display_name().to_string(),
                provider: role_config.provider.clone(),
                model: role_config.model.clone(),
                effort: role_config.effort.as_str().to_string(),
            }
        })
        .collect()
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
        default_model: info.default_model,
        wire_api: info.wire_api.to_string(),
        default_models: kind.default_models()?.iter().map(model_dto).collect(),
    })
}

fn provider_status(provider: &ProviderConfig) -> &'static str {
    if provider.bearer_token.is_some() {
        "Healthy"
    } else {
        "Needs setup"
    }
}

fn model_dto(model: &ModelConfig) -> ModelDto {
    ModelDto {
        slug: model.slug.clone(),
        display_name: model.display_name.clone(),
        description: model.description.clone(),
        context_window: model.context_window,
        max_context_window: model.max_context_window,
        auto_compact_token_limit: model.auto_compact_token_limit,
        default_temperature: model.default_temperature,
        max_output_tokens: model.max_output_tokens,
        currency: model.currency.clone(),
        input_price_per_mtok: model.input_price_per_mtok,
        output_price_per_mtok: model.output_price_per_mtok,
        cache_read_price_per_mtok: model.cache_read_price_per_mtok,
        reasoning_efforts: model.reasoning_efforts.clone(),
        capabilities: model
            .capabilities
            .iter()
            .map(capability_name)
            .map(str::to_string)
            .collect(),
        input_modalities: model
            .input_modalities
            .iter()
            .map(|modality| format!("{modality:?}").to_ascii_lowercase())
            .collect(),
        truncation_mode: format!("{:?}", model.truncation_policy.mode).to_ascii_lowercase(),
        truncation_limit: model.truncation_policy.limit,
    }
}

fn capability_name(capability: &ModelCapabilityConfig) -> &'static str {
    match capability {
        ModelCapabilityConfig::Streaming => "streaming",
        ModelCapabilityConfig::FunctionCalling => "function_calling",
        ModelCapabilityConfig::Vision => "vision",
        ModelCapabilityConfig::ParallelToolCalls => "parallel_tool_calls",
        ModelCapabilityConfig::Reasoning => "reasoning",
        ModelCapabilityConfig::WebSearch => "web_search",
        ModelCapabilityConfig::CustomTools => "custom_tools",
        ModelCapabilityConfig::FreeformTools => "freeform_tools",
    }
}

fn provider_edit(input: ProviderInput) -> CommandResult<ProviderEdit> {
    Ok(ProviderEdit {
        key: input.id,
        kind: provider_template_kind(&input.template_kind)?,
        name: input.name,
        base_url: Some(input.base_url),
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

fn role_edit(input: RoleInput) -> RoleEdit {
    RoleEdit {
        key: input.key,
        provider: input.provider,
        model: input.model,
        effort: input.effort,
    }
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

fn subagent_event_dtos(events: Vec<SubagentEventRecord>) -> Vec<SubagentEventDto> {
    events.into_iter().map(subagent_event_dto).collect()
}

fn subagent_event_dto(event: SubagentEventRecord) -> SubagentEventDto {
    SubagentEventDto {
        event_id: event.event_id,
        id: event.subagent_id,
        parent_id: event.parent_id,
        role: event.role,
        task: event.task,
        status: event.status,
        summary: event.summary,
        depth: event.depth,
        error: event.error,
        updated_at: event.created_at,
    }
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
        metadata: message.metadata,
    }
}

fn subagent_status_label(status: SubagentStatus) -> &'static str {
    match status {
        SubagentStatus::Queued => "queued",
        SubagentStatus::AwaitingApproval => "awaitingApproval",
        SubagentStatus::Running => "running",
        SubagentStatus::AwaitingToolApproval => "awaitingToolApproval",
        SubagentStatus::Succeeded => "succeeded",
        SubagentStatus::Failed => "failed",
        SubagentStatus::Denied => "denied",
    }
}

fn new_event_id(prefix: &str) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros();
    let seq = EVENT_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{now:x}-{seq:x}")
}

fn message_content_text(content: MessageContent) -> String {
    match content {
        MessageContent::Text(text) => text,
        MessageContent::MultiPart(parts) => {
            serde_json::to_string_pretty(&parts).unwrap_or_default()
        }
    }
}
