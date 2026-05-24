use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use pl_core::{
    ConfigStore, ProjectRecord, PureConfig, SessionRecord, StudioRuntime, ToolApprovalCallback,
    ToolApprovalDecision, ToolApprovalRequest,
};
use pl_protocol::{AgentEvent, MessageContent, MessageRole};
use slint::{ModelRc, SharedString, VecModel};
use tokio::sync::{Mutex, oneshot};

slint::include_modules!();

type ApprovalWaiters = Arc<Mutex<HashMap<String, oneshot::Sender<ToolApprovalDecision>>>>;

#[derive(Clone)]
struct AppState {
    studio: StudioRuntime,
    selected_project_id: Arc<Mutex<Option<String>>>,
    selected_session_id: Arc<Mutex<Option<String>>>,
    approvals: ApprovalWaiters,
}

fn main() -> Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    let studio = runtime.block_on(StudioRuntime::default_app())?;
    let app = MainWindow::new()?;
    let state = AppState {
        studio,
        selected_project_id: Arc::new(Mutex::new(None)),
        selected_session_id: Arc::new(Mutex::new(None)),
        approvals: Arc::new(Mutex::new(HashMap::new())),
    };

    app.set_config_toml(load_config_text(state.studio.config_store())?.into());
    app.set_active_page(0);
    app.set_status_text("Ready".into());
    install_callbacks(&app, runtime.handle().clone(), state.clone());
    runtime.block_on(bootstrap(&app, state.clone()))?;

    app.run()?;
    Ok(())
}

fn install_callbacks(app: &MainWindow, handle: tokio::runtime::Handle, state: AppState) {
    let weak = app.as_weak();
    let add_state = state.clone();
    let add_handle = handle.clone();
    app.on_add_project_dialog(move || {
        let Some(path) = rfd::FileDialog::new().pick_folder() else {
            return;
        };
        let weak = weak.clone();
        let state = add_state.clone();
        add_handle.spawn(async move {
            if let Err(error) = add_project_and_select(state, weak.clone(), path).await {
                set_status(&weak, format!("Add project failed: {error}"));
            }
        });
    });

    let weak = app.as_weak();
    let path_state = state.clone();
    let path_handle = handle.clone();
    app.on_add_project_path(move |path| {
        let path = path.to_string();
        if path.trim().is_empty() {
            return;
        }
        let weak = weak.clone();
        let state = path_state.clone();
        path_handle.spawn(async move {
            if let Err(error) =
                add_project_and_select(state, weak.clone(), PathBuf::from(path)).await
            {
                set_status(&weak, format!("Add project failed: {error}"));
            }
        });
    });

    let weak = app.as_weak();
    let select_state = state.clone();
    let select_handle = handle.clone();
    app.on_select_project(move |project_id| {
        let weak = weak.clone();
        let state = select_state.clone();
        let project_id = project_id.to_string();
        select_handle.spawn(async move {
            if let Err(error) = select_project(state, weak.clone(), project_id).await {
                set_status(&weak, format!("Select project failed: {error}"));
            }
        });
    });

    let weak = app.as_weak();
    let session_state = state.clone();
    let session_handle = handle.clone();
    app.on_new_session(move || {
        let weak = weak.clone();
        let state = session_state.clone();
        session_handle.spawn(async move {
            if let Err(error) = create_and_select_session(state, weak.clone(), "新会话").await {
                set_status(&weak, format!("New session failed: {error}"));
            }
        });
    });

    let weak = app.as_weak();
    let select_session_state = state.clone();
    let select_session_handle = handle.clone();
    app.on_select_session(move |session_id| {
        let weak = weak.clone();
        let state = select_session_state.clone();
        let session_id = session_id.to_string();
        select_session_handle.spawn(async move {
            if let Err(error) = select_session(state, weak.clone(), session_id).await {
                set_status(&weak, format!("Select session failed: {error}"));
            }
        });
    });

    let weak = app.as_weak();
    let prompt_state = state.clone();
    let prompt_handle = handle.clone();
    app.on_send_prompt(move |prompt| {
        let prompt = prompt.to_string();
        if prompt.trim().is_empty() {
            return;
        }
        if let Some(ui) = weak.upgrade() {
            ui.set_prompt_text("".into());
            ui.set_is_busy(true);
            ui.set_streaming_text("".into());
            ui.set_thinking_text("".into());
            ui.set_status_text("Running".into());
        }
        let weak = weak.clone();
        let state = prompt_state.clone();
        prompt_handle.spawn(async move {
            if let Err(error) = run_prompt(state, weak.clone(), prompt).await {
                set_busy(&weak, false);
                set_status(&weak, format!("Run failed: {error}"));
            }
        });
    });

    let weak = app.as_weak();
    app.on_show_chat(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_active_page(0);
        }
    });

    let weak = app.as_weak();
    app.on_show_settings(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_active_page(1);
        }
    });

    let weak = app.as_weak();
    let reload_state = state.clone();
    app.on_reload_config(
        move || match load_config_text(reload_state.studio.config_store()) {
            Ok(text) => {
                if let Some(ui) = weak.upgrade() {
                    ui.set_config_toml(text.into());
                    ui.set_status_text("Config reloaded".into());
                }
            }
            Err(error) => set_status(&weak, format!("Reload failed: {error}")),
        },
    );

    let weak = app.as_weak();
    let save_state = state.clone();
    app.on_save_config(move |content| {
        let content = content.to_string();
        match PureConfig::from_toml(&content)
            .and_then(|config| save_state.studio.config_store().save(&config))
        {
            Ok(()) => set_status(&weak, "Config saved".to_string()),
            Err(error) => set_status(&weak, format!("Config invalid: {error}")),
        }
    });

    let weak = app.as_weak();
    let approve_state = state.clone();
    let approve_handle = handle.clone();
    app.on_approve_tool(move |approval_id| {
        decide_tool(
            approve_handle.clone(),
            approve_state.clone(),
            weak.clone(),
            approval_id.to_string(),
            ToolApprovalDecision::Approved,
        );
    });

    let weak = app.as_weak();
    let deny_state = state;
    app.on_deny_tool(move |approval_id| {
        decide_tool(
            handle.clone(),
            deny_state.clone(),
            weak.clone(),
            approval_id.to_string(),
            ToolApprovalDecision::Denied {
                reason: "denied by user".to_string(),
            },
        );
    });
}

async fn bootstrap(app: &MainWindow, state: AppState) -> Result<()> {
    let mut projects = state.studio.list_projects().await?;
    if projects.is_empty()
        && let Ok(cwd) = std::env::current_dir()
    {
        let project = state.studio.open_project(cwd).await?;
        projects.push(project);
    }
    app.set_projects(project_rows(projects.clone()));
    if let Some(project) = projects.first() {
        *state.selected_project_id.lock().await = Some(project.id.clone());
        app.set_selected_project_id(project.id.clone().into());
        ensure_sessions_for_project(&state, app.as_weak(), project.id.clone()).await?;
    }
    Ok(())
}

async fn add_project_and_select(
    state: AppState,
    weak: slint::Weak<MainWindow>,
    path: PathBuf,
) -> Result<()> {
    if !path.is_dir() {
        anyhow::bail!("not a directory: {}", path.display());
    }
    let project = state.studio.open_project(path).await?;
    refresh_projects(&state, &weak).await?;
    select_project(state, weak, project.id).await
}

async fn select_project(
    state: AppState,
    weak: slint::Weak<MainWindow>,
    project_id: String,
) -> Result<()> {
    state
        .studio
        .store()
        .mark_project_opened(&project_id)
        .await?;
    *state.selected_project_id.lock().await = Some(project_id.clone());
    *state.selected_session_id.lock().await = None;
    let project_id_for_ui = project_id.clone();
    let weak_for_clear = weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = weak_for_clear.upgrade() {
            ui.set_selected_project_id(project_id_for_ui.into());
            ui.set_selected_session_id("".into());
            ui.set_messages(empty_messages());
            ui.set_streaming_text("".into());
            ui.set_thinking_text("".into());
        }
    });
    ensure_sessions_for_project(&state, weak, project_id).await
}

async fn ensure_sessions_for_project(
    state: &AppState,
    weak: slint::Weak<MainWindow>,
    project_id: String,
) -> Result<()> {
    let sessions = state.studio.ensure_project_sessions(&project_id).await?;
    let first_session = sessions.first().cloned();
    set_sessions(&weak, sessions);
    if let Some(session) = first_session {
        *state.selected_session_id.lock().await = Some(session.id.clone());
        let messages = state.studio.store().load_messages(&session.id).await?;
        let session_id = session.id;
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_selected_session_id(session_id.into());
                ui.set_messages(message_rows(messages));
            }
        });
    }
    Ok(())
}

async fn create_and_select_session(
    state: AppState,
    weak: slint::Weak<MainWindow>,
    title: &str,
) -> Result<()> {
    let project_id = state
        .selected_project_id
        .lock()
        .await
        .clone()
        .context("no project selected")?;
    let session = state.studio.create_session(&project_id, title).await?;
    refresh_sessions(&state, &weak, &project_id).await?;
    select_session(state, weak, session.id).await
}

async fn select_session(
    state: AppState,
    weak: slint::Weak<MainWindow>,
    session_id: String,
) -> Result<()> {
    *state.selected_session_id.lock().await = Some(session_id.clone());
    let messages = state.studio.store().load_messages(&session_id).await?;
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_selected_session_id(session_id.into());
            ui.set_messages(message_rows(messages));
            ui.set_streaming_text("".into());
            ui.set_thinking_text("".into());
        }
    });
    Ok(())
}

async fn run_prompt(state: AppState, weak: slint::Weak<MainWindow>, prompt: String) -> Result<()> {
    let session_id = state
        .selected_session_id
        .lock()
        .await
        .clone()
        .context("no session selected")?;
    let (event_tx, event_rx) = tokio::sync::broadcast::channel(256);
    let event_task = tokio::spawn(drain_events(event_rx, weak.clone()));
    let result = state
        .studio
        .run_prompt(
            &session_id,
            prompt,
            event_tx,
            approval_callback(state.approvals.clone(), weak.clone()),
        )
        .await;
    let _ = event_task.await;

    match result {
        Ok(outcome) => {
            let session_record = state
                .studio
                .store()
                .read_session(&session_id)
                .await?
                .context("selected session not found")?;
            set_messages(&weak, outcome.messages);
            refresh_sessions(&state, &weak, &session_record.project_id).await?;
            set_status(&weak, "Done".to_string());
        }
        Err(error) => {
            set_status(&weak, format!("Run failed: {error}"));
        }
    }
    set_busy(&weak, false);
    clear_streaming(&weak);
    Ok(())
}

fn approval_callback(
    approvals: ApprovalWaiters,
    weak: slint::Weak<MainWindow>,
) -> ToolApprovalCallback {
    Arc::new(move |request: ToolApprovalRequest| {
        let approvals = approvals.clone();
        let weak = weak.clone();
        Box::pin(async move {
            let (tx, rx) = oneshot::channel();
            approvals.lock().await.insert(request.id.clone(), tx);
            let arguments_json =
                serde_json::to_string_pretty(&request.arguments).unwrap_or_default();
            let approval_id = request.id.clone();
            let approval_text = format!(
                "Tool approval required\nname: {}\nworking directory: {}\narguments:\n{}",
                request.name,
                request.working_directory.as_deref().unwrap_or("(default)"),
                arguments_json
            );
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = weak.upgrade() {
                    ui.set_approval_visible(true);
                    ui.set_approval_id(approval_id.into());
                    ui.set_approval_text(approval_text.into());
                }
            });

            rx.await.unwrap_or_else(|_| ToolApprovalDecision::Denied {
                reason: "approval channel closed".to_string(),
            })
        })
    })
}

fn decide_tool(
    handle: tokio::runtime::Handle,
    state: AppState,
    weak: slint::Weak<MainWindow>,
    approval_id: String,
    decision: ToolApprovalDecision,
) {
    handle.spawn(async move {
        if let Some(sender) = state.approvals.lock().await.remove(&approval_id) {
            let _ = sender.send(decision);
        }
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_approval_visible(false);
                ui.set_approval_id("".into());
                ui.set_approval_text("".into());
            }
        });
    });
}

async fn drain_events(
    mut event_rx: tokio::sync::broadcast::Receiver<AgentEvent>,
    weak: slint::Weak<MainWindow>,
) {
    let mut text = String::new();
    let mut thinking = String::new();
    loop {
        let Ok(event) = event_rx.recv().await else {
            break;
        };
        match event {
            AgentEvent::TextDelta { content } => {
                text.push_str(&content);
                set_streaming(&weak, text.clone());
            }
            AgentEvent::ThinkingDelta { content } => {
                thinking.push_str(&content);
                set_thinking(&weak, thinking.clone());
            }
            AgentEvent::ToolApprovalRequested { name, .. } => {
                set_status(&weak, format!("Waiting for approval: {name}"));
            }
            AgentEvent::ToolApprovalGranted { name, .. } => {
                set_status(&weak, format!("Tool approved: {name}"));
            }
            AgentEvent::ToolApprovalDenied { name, .. } => {
                set_status(&weak, format!("Tool denied: {name}"));
            }
            AgentEvent::ToolCallComplete { name, .. } => {
                set_status(&weak, format!("Tool call: {name}"));
            }
            AgentEvent::Error { message, .. } => {
                set_status(&weak, format!("Error: {message}"));
            }
            AgentEvent::Done => break,
            AgentEvent::TurnStarted | AgentEvent::ToolCallDelta { .. } => {}
        }
    }
}

async fn refresh_projects(state: &AppState, weak: &slint::Weak<MainWindow>) -> Result<()> {
    let projects = state.studio.list_projects().await?;
    let weak = weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_projects(project_rows(projects));
        }
    });
    Ok(())
}

async fn refresh_sessions(
    state: &AppState,
    weak: &slint::Weak<MainWindow>,
    project_id: &str,
) -> Result<()> {
    let sessions = state.studio.store().list_sessions(project_id).await?;
    set_sessions(weak, sessions);
    Ok(())
}

fn set_sessions(weak: &slint::Weak<MainWindow>, sessions: Vec<SessionRecord>) {
    let weak = weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_sessions(session_rows(sessions));
        }
    });
}

fn set_messages(weak: &slint::Weak<MainWindow>, messages: Vec<pl_protocol::Message>) {
    let weak = weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_messages(message_rows(messages));
        }
    });
}

fn set_status(weak: &slint::Weak<MainWindow>, status: String) {
    let _ = slint::invoke_from_event_loop({
        let weak = weak.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_status_text(status.into());
            }
        }
    });
}

fn set_busy(weak: &slint::Weak<MainWindow>, busy: bool) {
    let _ = slint::invoke_from_event_loop({
        let weak = weak.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_is_busy(busy);
            }
        }
    });
}

fn set_streaming(weak: &slint::Weak<MainWindow>, content: String) {
    let _ = slint::invoke_from_event_loop({
        let weak = weak.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_streaming_text(content.into());
            }
        }
    });
}

fn set_thinking(weak: &slint::Weak<MainWindow>, content: String) {
    let _ = slint::invoke_from_event_loop({
        let weak = weak.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_thinking_text(content.into());
            }
        }
    });
}

fn clear_streaming(weak: &slint::Weak<MainWindow>) {
    let _ = slint::invoke_from_event_loop({
        let weak = weak.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_streaming_text("".into());
                ui.set_thinking_text("".into());
            }
        }
    });
}

fn project_rows(projects: Vec<ProjectRecord>) -> ModelRc<ProjectRow> {
    ModelRc::new(VecModel::from(
        projects
            .into_iter()
            .map(|project| ProjectRow {
                id: project.id.into(),
                name: project.name.into(),
                path: project.path.into(),
            })
            .collect::<Vec<_>>(),
    ))
}

fn session_rows(sessions: Vec<SessionRecord>) -> ModelRc<SessionRow> {
    ModelRc::new(VecModel::from(
        sessions
            .into_iter()
            .map(|session| SessionRow {
                id: session.id.into(),
                title: session.title.into(),
                updated_at: session.updated_at.to_string().into(),
            })
            .collect::<Vec<_>>(),
    ))
}

fn message_rows(messages: Vec<pl_protocol::Message>) -> ModelRc<ChatMessageRow> {
    ModelRc::new(VecModel::from(
        messages
            .into_iter()
            .map(|message| {
                let role = match message.role {
                    MessageRole::System => "system",
                    MessageRole::User => "user",
                    MessageRole::Assistant => "assistant",
                    MessageRole::Tool => "tool",
                };
                let content = match message.content {
                    MessageContent::Text(text) => text,
                    MessageContent::MultiPart(parts) => {
                        serde_json::to_string_pretty(&parts).unwrap_or_default()
                    }
                };
                ChatMessageRow {
                    role: SharedString::from(role),
                    content: content.into(),
                }
            })
            .collect::<Vec<_>>(),
    ))
}

fn empty_messages() -> ModelRc<ChatMessageRow> {
    ModelRc::new(VecModel::from(Vec::<ChatMessageRow>::new()))
}

fn load_config_text(store: &ConfigStore) -> pl_core::Result<String> {
    store.load_or_default()?.to_toml_pretty()
}
