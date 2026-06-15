use std::time::Duration;

use pl_core::StudioRuntime;
use pl_protocol::{
    AgentEvent, StudioEventEnvelope, StudioEventKind, StudioLspHealth, StudioMcpHealth,
};
use tauri::{AppHandle, Emitter};
use tokio::sync::broadcast::error::RecvError;

use crate::mappers::{
    agent_dto, agent_event_dto, load_session_runtime_dto, lsp_health_update_dto,
    mcp_health_update_dto,
};
use crate::state::AppState;

const MCP_PERIODIC_RECHECK: Duration = Duration::from_secs(300);

pub fn start_mcp_health_tasks(app: AppHandle, state: AppState) {
    let initial_state = state.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = initial_state.studio.reconcile_mcp_runtime().await {
            eprintln!("[pure-studio] initial MCP health check failed to start: {error}");
        }
    });

    let event_app = app.clone();
    let event_state = state.clone();
    let mut updates = event_state.studio.mcp_runtime().subscribe();
    tauri::async_runtime::spawn(async move {
        while let Ok(()) | Err(RecvError::Lagged(_)) = updates.recv().await {
            emit_mcp_health_update(&event_app, &event_state.studio).await;
        }
    });

    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(MCP_PERIODIC_RECHECK);
        interval.tick().await;
        loop {
            interval.tick().await;
            if let Err(error) = state.studio.recheck_mcp_runtime().await {
                eprintln!("[pure-studio] periodic MCP health check failed to start: {error}");
            }
        }
    });
}

pub fn start_lsp_health_tasks(app: AppHandle, state: AppState) {
    let event_app = app.clone();
    let event_state = state.clone();
    let mut updates = event_state.studio.lsp_runtime().subscribe();
    tauri::async_runtime::spawn(async move {
        while let Ok(()) | Err(RecvError::Lagged(_)) = updates.recv().await {
            emit_lsp_health_update(&event_app, &event_state.studio).await;
        }
    });
}

async fn emit_mcp_health_update(_app: &AppHandle, studio: &StudioRuntime) {
    match mcp_health_update_dto(studio).await {
        Ok(payload) => {
            let Ok(payload) = serde_json::to_value(payload) else {
                eprintln!("[pure-studio] failed to serialize MCP health update");
                return;
            };
            let _ = studio
                .events()
                .emit(
                    None,
                    None,
                    None,
                    StudioEventKind::McpHealthChanged {
                        health: StudioMcpHealth { payload },
                    },
                )
                .await;
        }
        Err(error) => {
            eprintln!(
                "[pure-studio] failed to build MCP health update: {}",
                error.message
            );
        }
    }
}

pub async fn emit_lsp_health_update(_app: &AppHandle, studio: &StudioRuntime) {
    match lsp_health_update_dto(studio).await {
        Ok(payload) => {
            let Ok(payload) = serde_json::to_value(payload) else {
                eprintln!("[pure-studio] failed to serialize LSP health update");
                return;
            };
            let _ = studio
                .events()
                .emit(
                    None,
                    None,
                    None,
                    StudioEventKind::LspHealthChanged {
                        health: StudioLspHealth { payload },
                    },
                )
                .await;
        }
        Err(error) => {
            eprintln!(
                "[pure-studio] failed to build LSP health update: {}",
                error.message
            );
        }
    }
}

pub fn start_studio_runtime_event_bridge(app: AppHandle, state: AppState) {
    let mut rx = state.studio.events().subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(event) => emit_studio_runtime_event(&app, event),
                Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => break,
            }
        }
    });
}

fn emit_studio_runtime_event(app: &AppHandle, event: StudioEventEnvelope) {
    let _ = app.emit("studio-runtime-event", event);
}

pub async fn drain_events(
    session_id: String,
    mut event_rx: tokio::sync::broadcast::Receiver<AgentEvent>,
    _app: AppHandle,
    studio: StudioRuntime,
) {
    loop {
        match event_rx.recv().await {
            Ok(event) => {
                let emitted = studio
                    .events()
                    .emit_agent_event(&session_id, event.clone())
                    .await
                    .ok()
                    .flatten();
                let agent = agent_for_event(&studio, &session_id, &event).await;
                let timeline_event = if matches!(
                    emitted.as_ref().map(|envelope| &envelope.kind),
                    Some(StudioEventKind::AgentTimelineChanged { .. })
                ) {
                    let event_id = emitted.as_ref().map(|envelope| envelope.event_id.clone());
                    if let Some(event_id) = event_id {
                        studio
                            .store()
                            .read_agent_event(&event_id)
                            .await
                            .ok()
                            .flatten()
                            .map(agent_event_dto)
                    } else {
                        None
                    }
                } else {
                    None
                };
                let session_runtime = if matches!(
                    event,
                    AgentEvent::AgentRuntimeUpdated { .. } | AgentEvent::SkillActivated { .. }
                ) {
                    load_session_runtime_dto(&studio, &session_id).await.ok()
                } else {
                    None
                };
                if let Some(agent) = &agent {
                    let _ = studio
                        .events()
                        .emit(
                            None,
                            Some(session_id.clone()),
                            None,
                            StudioEventKind::AgentChanged {
                                agent: agent.clone().into(),
                            },
                        )
                        .await;
                }
                if let Some(timeline_event) = &timeline_event
                    && let Ok(payload) = serde_json::to_value(timeline_event)
                {
                    let _ = studio
                        .events()
                        .emit(
                            None,
                            Some(session_id.clone()),
                            None,
                            StudioEventKind::AgentTimelineChanged {
                                event: pl_protocol::StudioAgentTimelineEvent { payload },
                            },
                        )
                        .await;
                }
                if let Some(session_runtime) = &session_runtime {
                    let _ = studio
                        .events()
                        .emit(
                            None,
                            Some(session_id.clone()),
                            None,
                            StudioEventKind::SessionRuntimeChanged {
                                runtime: session_runtime.clone().into(),
                            },
                        )
                        .await;
                }
            }
            Err(RecvError::Lagged(skipped)) => {
                let _ = studio.events().emit_stale(&session_id, skipped).await;
            }
            Err(RecvError::Closed) => {
                break;
            }
        }
    }
}

async fn agent_for_event(
    studio: &StudioRuntime,
    session_id: &str,
    event: &AgentEvent,
) -> Option<crate::dto::AgentDto> {
    match event {
        AgentEvent::AgentStateChanged { id, .. } => studio
            .store()
            .list_agents(session_id)
            .await
            .ok()
            .and_then(|agents| {
                agents
                    .into_iter()
                    .find(|agent| agent.id == *id)
                    .map(agent_dto)
            }),
        AgentEvent::AgentRuntimeUpdated { delta } if delta.agent_id != "agent-root" => studio
            .store()
            .list_agents(session_id)
            .await
            .ok()
            .and_then(|agents| {
                agents
                    .into_iter()
                    .find(|agent| agent.id == delta.agent_id)
                    .map(agent_dto)
            }),
        AgentEvent::TimelineItemStarted { .. }
        | AgentEvent::TimelineItemDelta { .. }
        | AgentEvent::TimelineItemCompleted { .. }
        | AgentEvent::TimelineItemFailed { .. }
        | AgentEvent::InteractionChanged { .. }
        | AgentEvent::AgentRuntimeUpdated { .. }
        | AgentEvent::SkillActivated { .. }
        | AgentEvent::CollabAgentSpawnBegin { .. }
        | AgentEvent::CollabAgentSpawnEnd { .. }
        | AgentEvent::CollabAgentInteractionBegin { .. }
        | AgentEvent::CollabAgentInteractionEnd { .. }
        | AgentEvent::CollabWaitingBegin { .. }
        | AgentEvent::CollabWaitingEnd { .. }
        | AgentEvent::CollabCloseBegin { .. }
        | AgentEvent::CollabCloseEnd { .. }
        | AgentEvent::TurnInterrupted { .. }
        | AgentEvent::TurnBudgetLimited { .. }
        | AgentEvent::Done
        | AgentEvent::Error { .. } => None,
    }
}
