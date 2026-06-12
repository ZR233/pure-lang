use std::path::PathBuf;

use anyhow::Context;
use pl_core::{
    CompileMode, PermissionMode, PureConfig, SessionHandoffStatus, StudioPromptOutcome,
    StudioRuntime, TimelineEventRecord, TurnOptions, TurnResultStatus, resolution_matches_kind,
};
use pl_protocol::{
    InteractionKind, InteractionPayload, InteractionRequest, InteractionResolution,
    InteractionScope, InteractionStatus, PlanConfirmationResolution, PlanLifecycleEvent,
    PlanLifecycleState, StudioTurnStatus, TimelineItem, TimelineItemKind, TraceEvent,
    TraceEventKind,
};
use tauri::{AppHandle, State};
use tokio_util::sync::CancellationToken;

use crate::dto::{
    BootstrapDto, ConfigDto, DiscoveredSkillsDto, InstructionsInput, McpSettingsInput,
    PlanLifecycleResponse, ProjectSelectionDto, ProviderSettingsInput, ProviderUsagesDto,
    ResolveInteractionResponse, RunPromptResponse, SessionSelectionDto, SessionStateDto,
    SessionTimelineDto, StopPromptResponse, StudioEventsDto, SubmitPromptResponse,
};
use crate::events::drain_events;
use crate::interactions::interaction_emitter;
use crate::mappers::{
    agent_dtos, agent_event_dtos, config_dto_for_studio, discovered_skills_dto,
    instructions_config, load_session_runtime_dto, lsp_health_update_dto,
    mcp_settings_to_builtin_states, mcp_settings_to_servers, plan_lifecycle_events_to_states,
    project_dtos, provider_settings_to_edit, provider_usages_dto, session_dtos,
    timeline_events_to_items, turn_result_status_label,
};
use crate::state::{AppState, CommandError, CommandResult};

const IMPLEMENT_PLAN_FRESH_CONTEXT_PREFIX: &str = "A previous agent produced the plan below to accomplish the user's task. Implement the plan in a fresh context. Treat the plan as the source of user intent, re-read files as needed, and carry the work through implementation and verification.";

struct PlanLifecycleChange {
    state: PlanLifecycleState,
    turn_id: Option<String>,
    reason: Option<String>,
}

struct PlanImplementationLifecycle {
    origin_session_id: String,
    plan_id: String,
}

#[tauri::command]
pub async fn bootstrap_studio(state: State<'_, AppState>) -> CommandResult<BootstrapDto> {
    let mut projects = state.studio.list_projects().await?;
    if projects.is_empty()
        && !state.studio.store().has_projects().await?
        && let Ok(cwd) = std::env::current_dir()
    {
        projects.push(state.studio.open_project(cwd).await?);
    }

    let mut selected_project_id = None;
    let mut sessions = Vec::new();
    let mut selected_session_id = None;
    let mut agent_events = Vec::new();
    let mut agents = Vec::new();
    let mut interactions = Vec::new();

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
            interactions = state
                .studio
                .store()
                .list_pending_interactions(&session.id)
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
        agent_events: agent_event_dtos(agent_events),
        agents: agent_dtos(agents),
        session_runtime,
        interactions,
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
pub async fn archive_project(
    project_id: String,
    selected_project_id: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<ProjectSelectionDto> {
    let active_session_ids = state
        .active_turns
        .lock()
        .await
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    for session_id in active_session_ids {
        if let Some(session) = state.studio.store().read_session(&session_id).await?
            && session.project_id == project_id
        {
            return Err(CommandError {
                message: "project has an active turn".to_string(),
            });
        }
    }
    for session_id in state
        .studio
        .store()
        .list_project_session_ids(&project_id)
        .await?
    {
        let emitter = interaction_emitter(state.studio.clone(), app.clone(), session_id.clone());
        state
            .studio
            .interactions()
            .cancel_session(&session_id, "project archived", emitter)
            .await?;
    }
    state
        .studio
        .store()
        .archive_project(&project_id)
        .await?
        .context("selected project not found")?;
    let projects = state.studio.list_projects().await?;
    let next_project_id = selected_project_id
        .filter(|id| id != &project_id && projects.iter().any(|project| project.id == *id))
        .or_else(|| projects.first().map(|project| project.id.clone()));
    if let Some(next_project_id) = next_project_id {
        state
            .studio
            .reconcile_lsp_runtime_for_project(&next_project_id)
            .await?;
        return project_selection_data(&state, Some(next_project_id)).await;
    }
    project_selection_data(&state, None).await
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
        interactions: Vec::new(),
    })
}

#[tauri::command]
pub async fn delete_session(
    session_id: String,
    selected_session_id: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<ProjectSelectionDto> {
    if state.active_turns.lock().await.contains_key(&session_id) {
        return Err(CommandError {
            message: "session has an active turn".to_string(),
        });
    }
    let emitter = interaction_emitter(state.studio.clone(), app.clone(), session_id.clone());
    state
        .studio
        .interactions()
        .cancel_session(&session_id, "session archived", emitter)
        .await?;
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
    let interactions = match &selected_session_id {
        Some(session_id) => {
            state
                .studio
                .store()
                .list_pending_interactions(session_id)
                .await?
        }
        None => Vec::new(),
    };
    let session_runtime = match selected_session_id.as_deref() {
        Some(session_id) => Some(load_session_runtime_dto(&state.studio, session_id).await?),
        None => None,
    };
    Ok(ProjectSelectionDto {
        selected_project_id: Some(project_id),
        projects: project_dtos(state.studio.store().list_projects().await?),
        sessions: session_dtos(sessions),
        selected_session_id,
        agent_events: agent_event_dtos(agent_events),
        agents: agent_dtos(agents),
        session_runtime,
        interactions,
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
        interactions: state
            .studio
            .store()
            .list_pending_interactions(&session_id)
            .await?,
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
    session_selection_response(&state.studio, &session_id).await
}

#[tauri::command]
pub async fn submit_prompt(
    session_id: String,
    prompt: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<SubmitPromptResponse> {
    submit_prompt_background(session_id, prompt, app, &state, None).await
}

#[tauri::command]
pub async fn resolve_interaction(
    interaction_id: String,
    resolution: InteractionResolution,
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<ResolveInteractionResponse> {
    let current = state
        .studio
        .store()
        .read_interaction(&interaction_id)
        .await?
        .context("interaction not found")?;
    let session_id = current.scope.session_id.clone();
    if !resolution_matches_kind(&current.kind, &resolution) {
        return Err(CommandError::from_display(
            "interaction resolution kind does not match interaction",
        ));
    }
    let emitter = interaction_emitter(state.studio.clone(), app.clone(), session_id.clone());

    match resolution {
        InteractionResolution::PlanConfirmation {
            decision,
            content,
            reason,
        } => {
            let InteractionPayload::PlanConfirmation { plan_id, .. } = &current.payload else {
                unreachable!("plan confirmation resolution was validated before resolving");
            };
            let (resolved, plan_lifecycle) = match decision {
                PlanConfirmationResolution::ImplementFreshContext => {
                    let resolution = InteractionResolution::PlanConfirmation {
                        decision: PlanConfirmationResolution::ImplementFreshContext,
                        content,
                        reason,
                    };
                    let handoff = state
                        .studio
                        .store()
                        .start_plan_implementation_handoff(&interaction_id, resolution)
                        .await?;
                    let _ = emitter(handoff.interaction.clone()).await;
                    if handoff.should_start_run {
                        let content = handoff.plan_content.trim().to_string();
                        if content.is_empty() {
                            return Err(CommandError::from_display("plan content is empty"));
                        }
                        let _ = state.studio.events().emit_handoff(&handoff.handoff).await?;
                        let prompt = format!("{IMPLEMENT_PLAN_FRESH_CONTEXT_PREFIX}\n\n{content}");
                        let _ = submit_prompt_background(
                            handoff.target_session.id.clone(),
                            prompt,
                            app,
                            &state,
                            Some(PlanImplementationLifecycle {
                                origin_session_id: handoff.origin_session.id.clone(),
                                plan_id: handoff.plan_id.clone(),
                            }),
                        )
                        .await?;
                    }
                    let plan_lifecycle = Some(
                        plan_lifecycle_response(&state.studio, &handoff.origin_session.id).await?,
                    );
                    (handoff.interaction, plan_lifecycle)
                }
                PlanConfirmationResolution::ContinuePlanning => {
                    if current.status != InteractionStatus::Pending {
                        return Ok(ResolveInteractionResponse {
                            session_id,
                            interaction: current,
                            plan_lifecycle: None,
                        });
                    }
                    let resolved = state
                        .studio
                        .interactions()
                        .resolve(
                            &interaction_id,
                            InteractionResolution::PlanConfirmation {
                                decision: PlanConfirmationResolution::ContinuePlanning,
                                content: content.clone(),
                                reason: reason.clone(),
                            },
                            emitter,
                        )
                        .await?;
                    let reason = reason
                        .or(content)
                        .filter(|value| !value.trim().is_empty())
                        .unwrap_or_else(|| "continue planning".to_string());
                    append_plan_lifecycle_events(
                        &state.studio,
                        &session_id,
                        plan_id,
                        vec![PlanLifecycleChange {
                            state: PlanLifecycleState::ContinuedPlanning,
                            turn_id: None,
                            reason: Some(reason),
                        }],
                    )
                    .await?;
                    let plan_lifecycle =
                        Some(plan_lifecycle_response(&state.studio, &session_id).await?);
                    (resolved, plan_lifecycle)
                }
                PlanConfirmationResolution::Dismiss => {
                    if current.status != InteractionStatus::Pending {
                        return Ok(ResolveInteractionResponse {
                            session_id,
                            interaction: current,
                            plan_lifecycle: None,
                        });
                    }
                    let resolved = state
                        .studio
                        .interactions()
                        .resolve(
                            &interaction_id,
                            InteractionResolution::PlanConfirmation {
                                decision: PlanConfirmationResolution::Dismiss,
                                content,
                                reason: reason.clone(),
                            },
                            emitter,
                        )
                        .await?;
                    let reason = reason
                        .filter(|value| !value.trim().is_empty())
                        .unwrap_or_else(|| "dismissed".to_string());
                    append_plan_lifecycle_events(
                        &state.studio,
                        &session_id,
                        plan_id,
                        vec![PlanLifecycleChange {
                            state: PlanLifecycleState::Dismissed,
                            turn_id: None,
                            reason: Some(reason),
                        }],
                    )
                    .await?;
                    let plan_lifecycle =
                        Some(plan_lifecycle_response(&state.studio, &session_id).await?);
                    (resolved, plan_lifecycle)
                }
            };
            Ok(ResolveInteractionResponse {
                session_id,
                interaction: resolved,
                plan_lifecycle,
            })
        }
        other_resolution => {
            if current.status != InteractionStatus::Pending {
                return Ok(ResolveInteractionResponse {
                    session_id,
                    interaction: current,
                    plan_lifecycle: None,
                });
            }
            let resolved = state
                .studio
                .interactions()
                .resolve(&interaction_id, other_resolution, emitter)
                .await?;
            Ok(ResolveInteractionResponse {
                session_id,
                interaction: resolved,
                plan_lifecycle: None,
            })
        }
    }
}

async fn run_prompt_inner(
    session_id: String,
    prompt: String,
    app: AppHandle,
    state: &AppState,
    lifecycle: Option<PlanImplementationLifecycle>,
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
    let emitter = interaction_emitter(state.studio.clone(), app.clone(), session_id.clone());
    let interaction_callback = state
        .studio
        .interactions()
        .callback(session_id.clone(), emitter.clone());
    let options = TurnOptions::default()
        .with_cancellation(cancellation_token.clone())
        .with_interaction_callback(interaction_callback.clone());
    let result = state
        .studio
        .run_prompt(
            &session_id,
            prompt,
            event_tx.clone(),
            interaction_callback,
            options,
        )
        .await;
    drop(event_tx);
    let _ = event_task.await;
    state.active_turns.lock().await.remove(&session_id);
    state
        .studio
        .interactions()
        .cancel_session(&session_id, "turn completed", emitter)
        .await?;

    match result {
        Ok(outcome) => {
            if let Some(lifecycle) = &lifecycle {
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
                    &lifecycle.origin_session_id,
                    &lifecycle.plan_id,
                    vec![PlanLifecycleChange {
                        state: lifecycle_state,
                        turn_id,
                        reason,
                    }],
                )
                .await?;
                let handoff_status = match outcome.result.status {
                    TurnResultStatus::Completed => SessionHandoffStatus::Completed,
                    TurnResultStatus::Aborted => SessionHandoffStatus::Cancelled,
                    TurnResultStatus::Errored => SessionHandoffStatus::Failed,
                };
                state
                    .studio
                    .store()
                    .set_plan_implementation_handoff_status(
                        &lifecycle.origin_session_id,
                        &lifecycle.plan_id,
                        handoff_status,
                    )
                    .await?;
            } else {
                maybe_create_plan_confirmation(&state.studio, &app, &session_id, &outcome).await?;
            }
            run_prompt_response(state, &session_id, outcome).await
        }
        Err(error) => {
            let message = error.to_string();
            if let Some(lifecycle) = &lifecycle {
                append_plan_lifecycle_events(
                    &state.studio,
                    &lifecycle.origin_session_id,
                    &lifecycle.plan_id,
                    vec![PlanLifecycleChange {
                        state: PlanLifecycleState::ImplementationFailed,
                        turn_id: None,
                        reason: Some(message.clone()),
                    }],
                )
                .await?;
                state
                    .studio
                    .store()
                    .set_plan_implementation_handoff_status(
                        &lifecycle.origin_session_id,
                        &lifecycle.plan_id,
                        SessionHandoffStatus::Failed,
                    )
                    .await?;
            }
            Err(CommandError { message })
        }
    }
}

async fn submit_prompt_background(
    session_id: String,
    prompt: String,
    app: AppHandle,
    state: &AppState,
    lifecycle: Option<PlanImplementationLifecycle>,
) -> CommandResult<SubmitPromptResponse> {
    if prompt.trim().is_empty() {
        return Err(CommandError::from_display("prompt is empty"));
    }
    if state.active_turns.lock().await.contains_key(&session_id) {
        return Err(CommandError::from_display(
            "session already has an active turn",
        ));
    }
    let turn_id = format!("turn-{}", uuid_like_suffix());
    state
        .studio
        .events()
        .emit_turn(&session_id, &turn_id, StudioTurnStatus::Queued, None)
        .await?;
    state
        .studio
        .events()
        .emit_turn(
            &session_id,
            &turn_id,
            StudioTurnStatus::ContextLoading,
            None,
        )
        .await?;
    let cursor = state
        .studio
        .store()
        .next_studio_event_sequence(&session_id)
        .await? as u64;
    let run_state = state.clone();
    let run_session_id = session_id.clone();
    let run_turn_id = turn_id.clone();
    tauri::async_runtime::spawn(async move {
        let _ = run_state
            .studio
            .events()
            .emit_turn(
                &run_session_id,
                &run_turn_id,
                StudioTurnStatus::WaitingForModel,
                None,
            )
            .await;
        let result =
            run_prompt_inner(run_session_id.clone(), prompt, app, &run_state, lifecycle).await;
        match result {
            Ok(response) => {
                let status = match response.turn_status.as_str() {
                    "completed" => StudioTurnStatus::Completed,
                    "aborted" if response.turn_abort_reason.as_deref() == Some("interrupted") => {
                        StudioTurnStatus::Cancelled
                    }
                    "aborted" | "errored" => StudioTurnStatus::Failed,
                    _ => StudioTurnStatus::Completed,
                };
                let reason = response.turn_error.or(response.turn_abort_reason);
                let _ = run_state
                    .studio
                    .events()
                    .emit_turn(&run_session_id, &run_turn_id, status, reason)
                    .await;
            }
            Err(error) => {
                let _ = run_state
                    .studio
                    .events()
                    .emit_turn(
                        &run_session_id,
                        &run_turn_id,
                        StudioTurnStatus::Failed,
                        Some(error.message),
                    )
                    .await;
            }
        }
    });
    Ok(SubmitPromptResponse {
        session_id,
        turn_id,
        cursor,
    })
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
    let emitter = interaction_emitter(state.studio.clone(), app, session_id.clone());
    state
        .studio
        .interactions()
        .cancel_session(&session_id, "interrupted by user", emitter)
        .await?;

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
    load_session_timeline_inner(&state, session_id, after_sequence, limit).await
}

async fn load_session_timeline_inner(
    state: &AppState,
    session_id: String,
    after_sequence: Option<i64>,
    limit: Option<i64>,
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
        session_id: session_id.clone(),
        items: timeline_events_to_items(&timeline_events),
        plan_states: plan_lifecycle_events_to_states(&timeline_events),
        interactions: state
            .studio
            .store()
            .list_pending_interactions(&session_id)
            .await?,
        next_sequence,
    })
}

#[tauri::command]
pub async fn load_studio_events(
    session_id: String,
    after_sequence: Option<i64>,
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> CommandResult<StudioEventsDto> {
    let events = state
        .studio
        .store()
        .load_studio_events(&session_id, after_sequence, limit)
        .await?;
    let next_sequence = state
        .studio
        .store()
        .next_studio_event_sequence(&session_id)
        .await? as u64;
    Ok(StudioEventsDto {
        session_id,
        events,
        next_sequence,
    })
}

#[tauri::command]
pub async fn load_session_state(
    session_id: String,
    state: State<'_, AppState>,
) -> CommandResult<SessionStateDto> {
    let session = state
        .studio
        .store()
        .read_session(&session_id)
        .await?
        .context("selected session not found")?;
    let timeline = load_session_timeline_inner(&state, session_id.clone(), None, None).await?;
    let events = state
        .studio
        .store()
        .load_studio_events(&session_id, None, None)
        .await?;
    let event_next_sequence = state
        .studio
        .store()
        .next_studio_event_sequence(&session_id)
        .await? as u64;
    Ok(SessionStateDto {
        session_id: session_id.clone(),
        session: session_dtos(vec![session.clone()])
            .into_iter()
            .next()
            .context("selected session not found")?,
        sessions: session_dtos(
            state
                .studio
                .store()
                .list_sessions(&session.project_id)
                .await?,
        ),
        agent_events: agent_event_dtos(state.studio.store().list_agent_events(&session_id).await?),
        agents: agent_dtos(state.studio.store().list_agents(&session_id).await?),
        session_runtime: load_session_runtime_dto(&state.studio, &session_id).await?,
        interactions: state
            .studio
            .store()
            .list_pending_interactions(&session_id)
            .await?,
        timeline,
        events,
        event_next_sequence,
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
        interactions: state
            .studio
            .store()
            .list_pending_interactions(session_id)
            .await?,
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

async fn session_selection_response(
    studio: &StudioRuntime,
    session_id: &str,
) -> CommandResult<SessionSelectionDto> {
    let session = studio
        .store()
        .read_session(session_id)
        .await?
        .context("selected session not found")?;
    let sessions = studio.store().list_sessions(&session.project_id).await?;
    let agent_events = studio.store().list_agent_events(session_id).await?;
    let agents = studio.store().list_agents(session_id).await?;
    Ok(SessionSelectionDto {
        session_runtime: Some(load_session_runtime_dto(studio, session_id).await?),
        session_id: session_id.to_string(),
        sessions: session_dtos(sessions),
        agent_events: agent_event_dtos(agent_events),
        agents: agent_dtos(agents),
        interactions: studio.store().list_pending_interactions(session_id).await?,
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

async fn maybe_create_plan_confirmation(
    studio: &StudioRuntime,
    app: &AppHandle,
    session_id: &str,
    outcome: &StudioPromptOutcome,
) -> CommandResult<()> {
    if outcome.result.mode != CompileMode::Plan
        || !matches!(outcome.result.status, TurnResultStatus::Completed)
    {
        return Ok(());
    }
    let Some(plan) = latest_completed_plan(&outcome.timeline_events) else {
        return Ok(());
    };
    if plan.content.trim().is_empty() {
        return Ok(());
    }
    let interaction_id = format!("plan-confirmation-{}", plan.item_id);
    if studio
        .store()
        .read_interaction(&interaction_id)
        .await?
        .is_some()
    {
        return Ok(());
    }
    let now = unix_seconds();
    let plan_id = plan.item_id.clone();
    let turn_id = plan.turn_id.clone();
    let content = plan.content.clone();
    let interaction = InteractionRequest {
        interaction_id,
        kind: InteractionKind::PlanConfirmation,
        status: InteractionStatus::Pending,
        scope: InteractionScope {
            session_id: session_id.to_string(),
            turn_id: turn_id.clone(),
            item_id: Some(plan_id.clone()),
            tool_id: None,
            agent_path: None,
        },
        payload: InteractionPayload::PlanConfirmation {
            plan_id: plan_id.clone(),
            content,
        },
        created_at: now,
        updated_at: now,
        resolved_at: None,
        resolution: None,
    };
    let emitter = interaction_emitter(studio.clone(), app.clone(), session_id.to_string());
    studio.interactions().create(interaction, emitter).await?;
    append_plan_lifecycle_events(
        studio,
        session_id,
        &plan_id,
        vec![PlanLifecycleChange {
            state: PlanLifecycleState::PendingConfirmation,
            turn_id: Some(turn_id),
            reason: None,
        }],
    )
    .await?;
    Ok(())
}

fn latest_completed_plan(events: &[TraceEvent]) -> Option<TimelineItem> {
    events.iter().rev().find_map(|trace| match &trace.kind {
        TraceEventKind::TimelineItemCompleted { item } if item.kind == TimelineItemKind::Plan => {
            Some(item.clone())
        }
        TraceEventKind::TimelineItemStarted { .. }
        | TraceEventKind::TimelineItemDelta { .. }
        | TraceEventKind::TimelineItemCompleted { .. }
        | TraceEventKind::TimelineItemFailed { .. }
        | TraceEventKind::PlanLifecycleChanged { .. }
        | TraceEventKind::InteractionChanged { .. }
        | TraceEventKind::SkillActivated { .. }
        | TraceEventKind::EnabledToolsRecorded { .. } => None,
    })
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
        | TraceEventKind::PlanLifecycleChanged { .. }
        | TraceEventKind::InteractionChanged { .. }
        | TraceEventKind::SkillActivated { .. }
        | TraceEventKind::EnabledToolsRecorded { .. } => None,
    })
}

fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn uuid_like_suffix() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros()
        .to_string()
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
pub async fn save_instructions_settings(
    input: InstructionsInput,
    state: State<'_, AppState>,
) -> CommandResult<ConfigDto> {
    let mut config = state.studio.config_store().load_or_default()?;
    config.instructions = instructions_config(input);
    config.validate()?;
    state.studio.config_store().save(&config)?;
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
    project_selection_data(state, Some(project_id)).await
}

async fn project_selection_data(
    state: &State<'_, AppState>,
    project_id: Option<String>,
) -> CommandResult<ProjectSelectionDto> {
    let sessions = match &project_id {
        Some(project_id) => state.studio.ensure_project_sessions(project_id).await?,
        None => Vec::new(),
    };
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
    let interactions = match selected_session_id.as_deref() {
        Some(session_id) => {
            state
                .studio
                .store()
                .list_pending_interactions(session_id)
                .await?
        }
        None => Vec::new(),
    };
    Ok(ProjectSelectionDto {
        selected_project_id: project_id,
        projects: project_dtos(state.studio.list_projects().await?),
        sessions: session_dtos(sessions),
        selected_session_id,
        agent_events: agent_event_dtos(agent_events),
        agents: agent_dtos(agents),
        session_runtime,
        interactions,
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
