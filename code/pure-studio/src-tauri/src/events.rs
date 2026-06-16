use std::time::Duration;

use pl_core::StudioRuntime;
use pl_protocol::{StudioEventEnvelope, StudioEventKind};
use tauri::{AppHandle, Emitter};
use tokio::sync::broadcast::error::RecvError;

use crate::mappers::{lsp_health_update_dto, mcp_health_update_dto};
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
        Ok(health) => {
            let _ = studio
                .events()
                .emit_live(
                    None,
                    None,
                    None,
                    StudioEventKind::McpHealthChanged {
                        health: health.into(),
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
        Ok(health) => {
            let _ = studio
                .events()
                .emit_live(
                    None,
                    None,
                    None,
                    StudioEventKind::LspHealthChanged {
                        health: health.into(),
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
                Err(RecvError::Lagged(skipped)) => {
                    emit_stale_for_active_sessions(&state, skipped).await;
                }
                Err(RecvError::Closed) => break,
            }
        }
    });
}

fn emit_studio_runtime_event(app: &AppHandle, event: StudioEventEnvelope) {
    let _ = app.emit("studio-runtime-event", event);
}

async fn emit_stale_for_active_sessions(state: &AppState, lagged_events: u64) {
    let projects = match state.studio.store().list_projects().await {
        Ok(projects) => projects,
        Err(error) => {
            eprintln!("[pure-studio] failed to list projects after lagged studio events: {error}");
            return;
        }
    };
    for project in projects {
        let sessions = match state.studio.store().list_sessions(&project.id).await {
            Ok(sessions) => sessions,
            Err(error) => {
                eprintln!(
                    "[pure-studio] failed to list project sessions after lagged studio events: {error}"
                );
                continue;
            }
        };
        for session in sessions {
            if let Err(error) = state
                .studio
                .events()
                .emit_stale(&session.id, lagged_events)
                .await
            {
                eprintln!("[pure-studio] failed to emit stale studio event: {error}");
            }
        }
    }
}
