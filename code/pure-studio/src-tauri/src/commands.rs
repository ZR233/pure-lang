use std::path::PathBuf;

use anyhow::Context;
use pl_core::{CompileMode, PureConfig, TimelineEventRecord, TurnOptions};
use pl_protocol::{TraceEvent, TraceEventKind};
use tauri::{AppHandle, Emitter, State};
use tokio_util::sync::CancellationToken;

use crate::approvals::{approval_callback, deny_session_approvals, resolve_tool_approval};
use crate::dto::{
    BootstrapDto, ConfigDto, DiscoveredSkillsDto, ProjectSelectionDto, PromptFailedPayload,
    ProviderSettingsInput, RunPromptResponse, SessionSelectionDto, SessionTimelineDto,
    StopPromptResponse,
};
use crate::events::drain_events;
use crate::mappers::{
    agent_dtos, agent_event_dtos, config_dto, discovered_skills_dto, load_session_runtime_dto,
    project_dtos, provider_settings_to_edit, session_dtos, timeline_events_to_items,
    turn_result_status_label,
};
use crate::state::{AppState, CommandError, CommandResult};

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
        config: config_dto(state.studio.config_store())?,
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

    let (event_tx, event_rx) = tokio::sync::broadcast::channel(256);
    let event_task = tauri::async_runtime::spawn(drain_events(
        session_id.clone(),
        event_rx,
        app.clone(),
        state.studio.clone(),
    ));
    let approval_callback =
        approval_callback(state.approvals.clone(), app.clone(), session_id.clone());
    let options = TurnOptions::default().with_cancellation(cancellation_token.clone());
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
                sessions: session_dtos(sessions),
                agent_events: agent_event_dtos(
                    state.studio.store().list_agent_events(&session_id).await?,
                ),
                agents: agent_dtos(state.studio.store().list_agents(&session_id).await?),
                session_runtime: load_session_runtime_dto(&state.studio, &session_id).await?,
                timeline_items: timeline_events_to_items(&outcome.timeline_events),
                timeline_next_sequence: state
                    .studio
                    .store()
                    .next_timeline_sequence(&session_id)
                    .await?,
                turn_status: turn_result_status_label(outcome.result.status).to_string(),
                turn_abort_reason: outcome
                    .result
                    .abort_reason
                    .map(|reason| reason.as_str().to_string()),
            };
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
        next_sequence,
    })
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
pub fn load_config(state: State<'_, AppState>) -> CommandResult<ConfigDto> {
    config_dto(state.studio.config_store())
}

#[tauri::command]
pub fn save_config(toml: String, state: State<'_, AppState>) -> CommandResult<ConfigDto> {
    let config = PureConfig::from_toml(&toml)?;
    state.studio.config_store().save(&config)?;
    config_dto(state.studio.config_store())
}

#[tauri::command]
pub fn save_provider_settings(
    input: ProviderSettingsInput,
    state: State<'_, AppState>,
) -> CommandResult<ConfigDto> {
    let current = state.studio.config_store().load_or_default()?;
    let edit = provider_settings_to_edit(input, &current)?;
    let config = edit.to_config(&current)?;
    state.studio.config_store().save(&config)?;
    config_dto(state.studio.config_store())
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
