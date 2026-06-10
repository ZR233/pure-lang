use std::path::PathBuf;

use anyhow::Context;
use pl_core::{
    CompileMode, PermissionMode, PureConfig, StudioPromptOutcome, StudioRuntime,
    TimelineEventRecord, TurnOptions, TurnResultStatus, UserInputResponse,
};
use pl_protocol::{
    PlanLifecycleEvent, PlanLifecycleState, TimelineItemKind, TraceEvent, TraceEventKind,
};
use tauri::{AppHandle, Emitter, State};
use tokio_util::sync::CancellationToken;

use crate::approvals::{approval_callback, deny_session_approvals, resolve_tool_approval};
use crate::dto::{
    BootstrapDto, ConfigDto, DiscoveredSkillsDto, McpSettingsInput, PlanLifecycleResponse,
    ProjectSelectionDto, PromptFailedPayload, ProviderSettingsInput, ProviderUsagesDto,
    RunPromptResponse, SessionSelectionDto, SessionTimelineDto, StopPromptResponse,
};
use crate::events::drain_events;
use crate::mappers::{
    agent_dtos, agent_event_dtos, config_dto_for_studio, discovered_skills_dto,
    load_session_runtime_dto, lsp_health_update_dto, mcp_settings_to_builtin_states,
    mcp_settings_to_servers, plan_lifecycle_events_to_states, project_dtos,
    provider_settings_to_edit, provider_usages_dto, session_dtos, timeline_events_to_items,
    turn_result_status_label,
};
use crate::state::{AppState, CommandError, CommandResult};
use crate::user_input::{cancel_session_user_inputs, resolve_user_input, user_input_callback};

const IMPLEMENT_PLAN_PROMPT_PREFIX: &str = "PLEASE IMPLEMENT THIS PLAN:";

struct PlanLifecycleChange {
    state: PlanLifecycleState,
    turn_id: Option<String>,
    reason: Option<String>,
}

#[tauri::command]
pub async fn bootstrap_studio(state: State<'_, AppState>) -> CommandResult<BootstrapDto> {
    let mut projects = state.studio.list_projects().await?;
    if projects.is_empty()
        && let Ok(cwd) = std::env::current_dir()
    {
        projects.push(state.studio.open_project(cwd).await?);
    }

    let mut selected_project_id = None;
    let mut sessions = Vec::new();
    let mut selected_session_id = None;
    let mut agent_events = Vec::new();
    let mut agents = Vec::new();

    if let Some(project) = projects.first() {
        selected_project_id = Some(project.id.clone());
        state
            .studio
            .reconcile_lsp_runtime_for_project(&project.id)
            .await?;
        sessions = state.studio.ensure_project_sessions(&project.id).await?;
        if let Some(session) = sessions.first() {
            selected_session_id = Some(session.id.clone());
            agent_events = state.studio.store().list_agent_events(&session.id).await?;
            agents = state.studio.store().list_agents(&session.id).await?;
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
        agent_events: agent_event_dtos(agent_events),
        agents: agent_dtos(agents),
        session_runtime,
        lsp_health: lsp_health_update_dto(&state.studio).await?,
        config: config_dto_for_studio(&state.studio).await?,
    })
}

#[tauri::command]
pub async fn open_project(
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
pub async fn select_project(
    project_id: String,
    state: State<'_, AppState>,
) -> CommandResult<ProjectSelectionDto> {
    select_project_data(&state, project_id).await
}

#[tauri::command]
pub async fn create_session(
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
        agent_events: Vec::new(),
        agents: Vec::new(),
        session_runtime: Some(load_session_runtime_dto(&state.studio, &session.id).await?),
    })
}

#[tauri::command]
pub async fn delete_session(
    session_id: String,
    selected_session_id: Option<String>,
    state: State<'_, AppState>,
) -> CommandResult<ProjectSelectionDto> {
    if state.active_turns.lock().await.contains_key(&session_id) {
        return Err(CommandError {
            message: "session has an active turn".to_string(),
        });
    }
    let archived = state
        .studio
        .store()
        .archive_session(&session_id)
        .await?
        .context("selected session not found")?;
    let project_id = archived.project_id;
    let sessions = state.studio.store().list_sessions(&project_id).await?;
    let selected_session_id = selected_session_id
        .filter(|id| id != &session_id && sessions.iter().any(|session| session.id == *id))
        .or_else(|| sessions.first().map(|session| session.id.clone()));
    let agent_events = match &selected_session_id {
        Some(session_id) => state.studio.store().list_agent_events(session_id).await?,
        None => Vec::new(),
    };
    let agents = match &selected_session_id {
        Some(session_id) => state.studio.store().list_agents(session_id).await?,
        None => Vec::new(),
    };
    let session_runtime = match selected_session_id.as_deref() {
        Some(session_id) => Some(load_session_runtime_dto(&state.studio, session_id).await?),
        None => None,
    };
    Ok(ProjectSelectionDto {
        project_id,
        projects: project_dtos(state.studio.store().list_projects().await?),
        sessions: session_dtos(sessions),
        selected_session_id,
        agent_events: agent_event_dtos(agent_events),
        agents: agent_dtos(agents),
        session_runtime,
        lsp_health: lsp_health_update_dto(&state.studio).await?,
    })
}

#[tauri::command]
pub async fn select_session(
    session_id: String,
    state: State<'_, AppState>,
) -> CommandResult<SessionSelectionDto> {
    let agent_events = state.studio.store().list_agent_events(&session_id).await?;
    let agents = state.studio.store().list_agents(&session_id).await?;
    Ok(SessionSelectionDto {
        session_runtime: Some(load_session_runtime_dto(&state.studio, &session_id).await?),
        session_id,
        sessions: Vec::new(),
        agent_events: agent_event_dtos(agent_events),
        agents: agent_dtos(agents),
    })
}

#[tauri::command]
pub async fn set_session_mode(
    session_id: String,
    mode: String,
    state: State<'_, AppState>,
) -> CommandResult<SessionSelectionDto> {
    let compile_mode = CompileMode::from_label(&mode);
    state
        .studio
        .store()
        .set_session_mode(&session_id, compile_mode)
        .await?;
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
    let agent_events = state.studio.store().list_agent_events(&session_id).await?;
    let agents = state.studio.store().list_agents(&session_id).await?;
    Ok(SessionSelectionDto {
        session_runtime: Some(load_session_runtime_dto(&state.studio, &session_id).await?),
        session_id,
        sessions: session_dtos(sessions),
        agent_events: agent_event_dtos(agent_events),
        agents: agent_dtos(agents),
    })
}

#[tauri::command]
pub async fn run_prompt(
    session_id: String,
    prompt: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<RunPromptResponse> {
    run_prompt_inner(session_id, prompt, app, &state, None).await
}

#[tauri::command]
pub async fn implement_plan(
    session_id: String,
    plan_id: String,
    content: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<RunPromptResponse> {
    let plan_id = plan_id.trim().to_string();
    if plan_id.is_empty() {
        return Err(CommandError::from_display("plan id is empty"));
    }
    let content = content.trim().to_string();
    if content.is_empty() {
        return Err(CommandError::from_display("plan content is empty"));
    }
    let prompt = format!("{IMPLEMENT_PLAN_PROMPT_PREFIX}\n\n{content}");
    run_prompt_inner(session_id, prompt, app, &state, Some(plan_id)).await
}

#[tauri::command]
pub async fn dismiss_plan(
    session_id: String,
    plan_id: String,
    reason: Option<String>,
    state: State<'_, AppState>,
) -> CommandResult<PlanLifecycleResponse> {
    let plan_id = plan_id.trim().to_string();
    if plan_id.is_empty() {
        return Err(CommandError::from_display("plan id is empty"));
    }
    let reason = reason
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "dismissed".to_string());
    append_plan_lifecycle_events(
        &state.studio,
        &session_id,
        &plan_id,
        vec![PlanLifecycleChange {
            state: PlanLifecycleState::Dismissed,
            turn_id: None,
            reason: Some(reason),
        }],
    )
    .await?;
    plan_lifecycle_response(&state.studio, &session_id).await
}

async fn run_prompt_inner(
    session_id: String,
    prompt: String,
    app: AppHandle,
    state: &AppState,
    lifecycle_plan_id: Option<String>,
) -> CommandResult<RunPromptResponse> {
    if prompt.trim().is_empty() {
        return Err(CommandError::from_display("prompt is empty"));
    }

    let cancellation_token = CancellationToken::new();
    {
        let mut active_turns = state.active_turns.lock().await;
        if active_turns.contains_key(&session_id) {
            return Err(CommandError::from_display(
                "session already has an active turn",
            ));
        }
        active_turns.insert(session_id.clone(), cancellation_token.clone());
    }

    if let Some(plan_id) = &lifecycle_plan_id {
        let setup_result = async {
            state
                .studio
                .store()
                .set_session_mode(&session_id, CompileMode::Auto)
                .await?;
            append_plan_lifecycle_events(
                &state.studio,
                &session_id,
                plan_id,
                vec![
                    PlanLifecycleChange {
                        state: PlanLifecycleState::Accepted,
                        turn_id: None,
                        reason: None,
                    },
                    PlanLifecycleChange {
                        state: PlanLifecycleState::Implementing,
                        turn_id: None,
                        reason: None,
                    },
                ],
            )
            .await
        }
        .await;
        if let Err(error) = setup_result {
            state.active_turns.lock().await.remove(&session_id);
            return Err(error);
        }
    }

    let (event_tx, event_rx) = tokio::sync::broadcast::channel(256);
    let event_task = tauri::async_runtime::spawn(drain_events(
        session_id.clone(),
        event_rx,
        app.clone(),
        state.studio.clone(),
    ));
    let approval_callback =
        approval_callback(state.approvals.clone(), app.clone(), session_id.clone());
    let user_input_callback =
        user_input_callback(state.user_inputs.clone(), app.clone(), session_id.clone());
    let options = TurnOptions::default()
        .with_cancellation(cancellation_token.clone())
        .with_user_input_callback(user_input_callback);
    let result = state
        .studio
        .run_prompt(
            &session_id,
            prompt,
            event_tx.clone(),
            approval_callback,
            options,
        )
        .await;
    drop(event_tx);
    let _ = event_task.await;
    state.active_turns.lock().await.remove(&session_id);
    cancel_session_user_inputs(&session_id, &app, state.user_inputs.clone()).await;

    match result {
        Ok(outcome) => {
            if let Some(plan_id) = &lifecycle_plan_id {
                let turn_id = first_turn_id(&outcome.timeline_events);
                let (lifecycle_state, reason) = match outcome.result.status {
                    TurnResultStatus::Completed => (PlanLifecycleState::Implemented, None),
                    TurnResultStatus::Aborted => (
                        PlanLifecycleState::ImplementationFailed,
                        outcome
                            .result
                            .abort_reason
                            .map(|reason| reason.as_str().to_string())
                            .or_else(|| Some("turn aborted".to_string())),
                    ),
                    TurnResultStatus::Errored => (
                        PlanLifecycleState::ImplementationFailed,
                        outcome
                            .result
                            .error
                            .clone()
                            .or_else(|| Some("turn errored".to_string())),
                    ),
                };
                append_plan_lifecycle_events(
                    &state.studio,
                    &session_id,
                    plan_id,
                    vec![PlanLifecycleChange {
                        state: lifecycle_state,
                        turn_id,
                        reason,
                    }],
                )
                .await?;
            }
            run_prompt_response(state, &session_id, outcome).await
        }
        Err(error) => {
            let message = error.to_string();
            if let Some(plan_id) = &lifecycle_plan_id {
                append_plan_lifecycle_events(
                    &state.studio,
                    &session_id,
                    plan_id,
                    vec![PlanLifecycleChange {
                        state: PlanLifecycleState::ImplementationFailed,
                        turn_id: None,
                        reason: Some(message.clone()),
                    }],
                )
                .await?;
            }
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
pub async fn stop_prompt(
    session_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<StopPromptResponse> {
    let token = state.active_turns.lock().await.get(&session_id).cloned();
    let Some(token) = token else {
        return Ok(StopPromptResponse {
            session_id,
            stopped: false,
        });
    };

    token.cancel();
    deny_session_approvals(
        &session_id,
        "interrupted by user",
        &app,
        state.approvals.clone(),
    )
    .await;
    cancel_session_user_inputs(&session_id, &app, state.user_inputs.clone()).await;

    Ok(StopPromptResponse {
        session_id,
        stopped: true,
    })
}

#[tauri::command]
pub async fn load_session_timeline(
    session_id: String,
    after_sequence: Option<i64>,
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> CommandResult<SessionTimelineDto> {
    let records = state
        .studio
        .store()
        .load_timeline_events(&session_id, after_sequence, limit)
        .await?;
    let next_sequence = state
        .studio
        .store()
        .next_timeline_sequence(&session_id)
        .await?;
    let timeline_events = timeline_records_to_trace_events(&records)?;
    Ok(SessionTimelineDto {
        session_id,
        items: timeline_events_to_items(&timeline_events),
        plan_states: plan_lifecycle_events_to_states(&timeline_events),
        next_sequence,
    })
}

async fn run_prompt_response(
    state: &AppState,
    session_id: &str,
    outcome: StudioPromptOutcome,
) -> CommandResult<RunPromptResponse> {
    let session = state
        .studio
        .store()
        .read_session(session_id)
        .await?
        .context("selected session not found")?;
    let sessions = state
        .studio
        .store()
        .list_sessions(&session.project_id)
        .await?;
    Ok(RunPromptResponse {
        session_id: session_id.to_string(),
        sessions: session_dtos(sessions),
        agent_events: agent_event_dtos(state.studio.store().list_agent_events(session_id).await?),
        agents: agent_dtos(state.studio.store().list_agents(session_id).await?),
        session_runtime: load_session_runtime_dto(&state.studio, session_id).await?,
        timeline_items: timeline_events_to_items(&outcome.timeline_events),
        plan_states: load_plan_states(&state.studio, session_id).await?,
        timeline_next_sequence: state
            .studio
            .store()
            .next_timeline_sequence(session_id)
            .await?,
        turn_status: turn_result_status_label(outcome.result.status).to_string(),
        turn_abort_reason: outcome
            .result
            .abort_reason
            .map(|reason| reason.as_str().to_string()),
        turn_error: outcome.result.error.clone(),
    })
}

async fn plan_lifecycle_response(
    studio: &StudioRuntime,
    session_id: &str,
) -> CommandResult<PlanLifecycleResponse> {
    Ok(PlanLifecycleResponse {
        session_id: session_id.to_string(),
        plan_states: load_plan_states(studio, session_id).await?,
        timeline_next_sequence: studio.store().next_timeline_sequence(session_id).await?,
    })
}

async fn load_plan_states(
    studio: &StudioRuntime,
    session_id: &str,
) -> CommandResult<Vec<crate::dto::PlanStateDto>> {
    let records = studio
        .store()
        .load_timeline_events(session_id, None, None)
        .await?;
    let timeline_events = timeline_records_to_trace_events(&records)?;
    Ok(plan_lifecycle_events_to_states(&timeline_events))
}

async fn append_plan_lifecycle_events(
    studio: &StudioRuntime,
    session_id: &str,
    plan_id: &str,
    changes: Vec<PlanLifecycleChange>,
) -> CommandResult<()> {
    if changes.is_empty() {
        return Ok(());
    }
    let mut sequence = studio.store().next_timeline_sequence(session_id).await?;
    let now = unix_seconds();
    let events = changes
        .into_iter()
        .map(|change| {
            let event = TraceEvent {
                session_id: session_id.to_string(),
                sequence,
                timestamp: now,
                kind: TraceEventKind::PlanLifecycleChanged {
                    event: PlanLifecycleEvent {
                        plan_id: plan_id.to_string(),
                        state: change.state,
                        turn_id: change.turn_id,
                        reason: change.reason,
                        updated_at: now,
                    },
                },
            };
            sequence += 1;
            event
        })
        .collect::<Vec<_>>();
    studio.store().append_timeline_events(&events).await?;
    Ok(())
}

fn first_turn_id(events: &[TraceEvent]) -> Option<String> {
    events.iter().find_map(|trace| match &trace.kind {
        TraceEventKind::TimelineItemStarted { item }
        | TraceEventKind::TimelineItemCompleted { item }
        | TraceEventKind::TimelineItemFailed { item, .. }
            if item.kind == TimelineItemKind::Turn =>
        {
            Some(item.turn_id.clone())
        }
        TraceEventKind::TimelineItemDelta { event } if event.kind == TimelineItemKind::Turn => {
            Some(event.turn_id.clone())
        }
        TraceEventKind::TimelineItemStarted { .. }
        | TraceEventKind::TimelineItemCompleted { .. }
        | TraceEventKind::TimelineItemFailed { .. }
        | TraceEventKind::TimelineItemDelta { .. }
        | TraceEventKind::PlanLifecycleChanged { .. } => None,
    })
}

fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn timeline_records_to_trace_events(
    records: &[TimelineEventRecord],
) -> CommandResult<Vec<TraceEvent>> {
    records
        .iter()
        .map(|record| {
            let kind: TraceEventKind =
                serde_json::from_str(&record.payload_json).map_err(|error| {
                    CommandError::from_display(format!(
                        "failed to parse timeline event {} for session {} at sequence {}: {error}",
                        record.id, record.session_id, record.sequence
                    ))
                })?;
            Ok(TraceEvent {
                session_id: record.session_id.clone(),
                sequence: record.sequence as u64,
                timestamp: record.created_at,
                kind,
            })
        })
        .collect()
}

#[tauri::command]
pub async fn approve_tool(
    approval_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<()> {
    resolve_tool_approval(
        approval_id,
        pl_core::ToolApprovalDecision::Approved,
        app,
        state.approvals.clone(),
    )
    .await;
    Ok(())
}

#[tauri::command]
pub async fn deny_tool(
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
        pl_core::ToolApprovalDecision::Denied { reason },
        app,
        state.approvals.clone(),
    )
    .await;
    Ok(())
}

#[tauri::command]
pub async fn answer_user_input(
    request_id: String,
    response: UserInputResponse,
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<()> {
    resolve_user_input(request_id, response, app, state.user_inputs.clone()).await;
    Ok(())
}

#[tauri::command]
pub async fn load_config(state: State<'_, AppState>) -> CommandResult<ConfigDto> {
    config_dto_for_studio(&state.studio).await
}

#[tauri::command]
pub async fn load_provider_usages(state: State<'_, AppState>) -> CommandResult<ProviderUsagesDto> {
    Ok(provider_usages_dto(state.studio.provider_usages().await?))
}

#[tauri::command]
pub async fn save_config(toml: String, state: State<'_, AppState>) -> CommandResult<ConfigDto> {
    let mut config = PureConfig::from_toml(&toml)?;
    config.runtime.active_mcp_servers = pl_core::active_mcp_server_names(&config);
    state.studio.config_store().save(&config)?;
    state.studio.reconcile_mcp_runtime().await?;
    config_dto_for_studio(&state.studio).await
}

#[tauri::command]
pub async fn save_provider_settings(
    input: ProviderSettingsInput,
    state: State<'_, AppState>,
) -> CommandResult<ConfigDto> {
    let current = state.studio.config_store().load_or_default()?;
    let edit = provider_settings_to_edit(input, &current)?;
    let mut config = edit.to_config(&current)?;
    config.runtime.active_mcp_servers = pl_core::active_mcp_server_names(&config);
    state.studio.config_store().save(&config)?;
    state.studio.reconcile_mcp_runtime().await?;
    config_dto_for_studio(&state.studio).await
}

#[tauri::command]
pub async fn save_permission_mode(
    mode: String,
    state: State<'_, AppState>,
) -> CommandResult<ConfigDto> {
    let mut config = state.studio.config_store().load_or_default()?;
    config.runtime.permission_mode = PermissionMode::from_label(&mode);
    state.studio.config_store().save(&config)?;
    config_dto_for_studio(&state.studio).await
}

#[tauri::command]
pub async fn save_mcp_settings(
    input: McpSettingsInput,
    state: State<'_, AppState>,
) -> CommandResult<ConfigDto> {
    let mut config = state.studio.config_store().load_or_default()?;
    config.builtin_mcp_servers = mcp_settings_to_builtin_states(&input, &config);
    config.mcp_servers = mcp_settings_to_servers(input)?;
    pl_core::normalize_builtin_mcp_server_states(&mut config);
    config.runtime.active_mcp_servers = pl_core::active_mcp_server_names(&config);
    config.validate()?;
    state.studio.config_store().save(&config)?;
    state.studio.reconcile_mcp_runtime().await?;
    config_dto_for_studio(&state.studio).await
}

#[tauri::command]
pub async fn list_discovered_skills(
    project_id: String,
    state: State<'_, AppState>,
) -> CommandResult<DiscoveredSkillsDto> {
    let catalog = state.studio.discovered_skills(&project_id).await?;
    Ok(discovered_skills_dto(catalog))
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
    state
        .studio
        .reconcile_lsp_runtime_for_project(&project_id)
        .await?;
    let sessions = state.studio.ensure_project_sessions(&project_id).await?;
    let selected_session_id = sessions.first().map(|session| session.id.clone());
    let agent_events = match &selected_session_id {
        Some(session_id) => state.studio.store().list_agent_events(session_id).await?,
        None => Vec::new(),
    };
    let agents = match &selected_session_id {
        Some(session_id) => state.studio.store().list_agents(session_id).await?,
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
        agent_events: agent_event_dtos(agent_events),
        agents: agent_dtos(agents),
        session_runtime,
        lsp_health: lsp_health_update_dto(&state.studio).await?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeline_record_parse_error_reports_session_and_sequence() {
        let records = vec![TimelineEventRecord {
            id: "event-1".to_string(),
            session_id: "session-1".to_string(),
            sequence: 42,
            created_at: 10,
            kind: "TimelineItemStarted".to_string(),
            payload_json: "{not valid json".to_string(),
        }];

        let error = timeline_records_to_trace_events(&records).unwrap_err();

        assert!(error.message.contains("event-1"));
        assert!(error.message.contains("session-1"));
        assert!(error.message.contains("sequence 42"));
    }
}
