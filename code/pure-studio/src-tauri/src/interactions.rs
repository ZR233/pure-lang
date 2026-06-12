use std::sync::Arc;

use anyhow::{Context, Result};
use pl_core::{
    InteractionCallback, InteractionChangedEvent, InteractionKind, InteractionRequest,
    InteractionResolution, InteractionStatus, PlanConfirmationResolution, StudioRuntime,
    ToolApprovalResolution,
};
use tauri::{AppHandle, Emitter};
use tokio::sync::oneshot;

use crate::dto::InteractionChangedPayload;
use crate::state::{InteractionWaiter, InteractionWaiters};

pub fn interaction_callback(
    waiters: InteractionWaiters,
    app: AppHandle,
    studio: StudioRuntime,
    session_id: String,
) -> InteractionCallback {
    Arc::new(move |request: InteractionRequest| {
        let waiters = waiters.clone();
        let app = app.clone();
        let studio = studio.clone();
        let session_id = session_id.clone();
        Box::pin(async move {
            let mut request = request;
            request.scope.session_id = session_id.clone();
            if request.scope.turn_id.trim().is_empty() {
                request.scope.turn_id = request.interaction_id.clone();
            }
            let (tx, rx) = oneshot::channel();
            let interaction_id = request.interaction_id.clone();
            waiters
                .lock()
                .await
                .insert(interaction_id.clone(), InteractionWaiter { sender: tx });
            if let Err(error) = persist_and_emit(&studio, &app, &request).await {
                waiters.lock().await.remove(&interaction_id);
                eprintln!("[pure-studio] failed to persist interaction: {error}");
                return cancelled_resolution(&request.kind, "interaction persistence failed");
            }
            rx.await.unwrap_or_else(|_| {
                cancelled_resolution(&request.kind, "interaction channel closed")
            })
        })
    })
}

pub async fn resolve_interaction_waiter(
    studio: &StudioRuntime,
    app: &AppHandle,
    waiters: InteractionWaiters,
    interaction_id: &str,
    resolution: InteractionResolution,
) -> Result<InteractionRequest> {
    let mut interaction = studio
        .store()
        .read_interaction(interaction_id)
        .await?
        .context("interaction not found")?;
    if interaction.status != InteractionStatus::Pending {
        return Ok(interaction);
    }
    if !resolution_matches_kind(&interaction.kind, &resolution) {
        anyhow::bail!("interaction resolution kind mismatch");
    }
    let now = unix_seconds();
    interaction.status = InteractionStatus::Resolved;
    interaction.updated_at = now;
    interaction.resolved_at = Some(now);
    interaction.resolution = Some(resolution);
    persist_and_emit(studio, app, &interaction).await?;
    if let Some(waiter) = waiters.lock().await.remove(interaction_id) {
        let Some(resolution) = interaction.resolution.clone() else {
            return Ok(interaction);
        };
        let _ = waiter.sender.send(resolution);
    }
    Ok(interaction)
}

pub async fn cancel_session_interactions(
    studio: &StudioRuntime,
    app: &AppHandle,
    waiters: InteractionWaiters,
    session_id: &str,
    reason: &str,
) {
    let pending = match studio.store().list_pending_interactions(session_id).await {
        Ok(interactions) => interactions,
        Err(error) => {
            eprintln!("[pure-studio] failed to load pending interactions: {error}");
            return;
        }
    };

    for mut interaction in pending {
        let resolution = cancelled_resolution(&interaction.kind, reason);
        let now = unix_seconds();
        interaction.status = InteractionStatus::Cancelled;
        interaction.updated_at = now;
        interaction.resolved_at = Some(now);
        interaction.resolution = Some(resolution.clone());
        if let Err(error) = persist_and_emit(studio, app, &interaction).await {
            eprintln!("[pure-studio] failed to persist cancelled interaction: {error}");
            continue;
        }
        if let Some(waiter) = waiters.lock().await.remove(&interaction.interaction_id) {
            let _ = waiter.sender.send(resolution);
        }
    }
}

pub fn resolution_matches_kind(kind: &InteractionKind, resolution: &InteractionResolution) -> bool {
    match (kind, resolution) {
        (InteractionKind::UserInput, InteractionResolution::UserInput { .. })
        | (InteractionKind::ToolApproval, InteractionResolution::ToolApproval { .. })
        | (InteractionKind::PlanConfirmation, InteractionResolution::PlanConfirmation { .. }) => {
            true
        }
        (InteractionKind::UserInput, InteractionResolution::ToolApproval { .. })
        | (InteractionKind::UserInput, InteractionResolution::PlanConfirmation { .. })
        | (InteractionKind::ToolApproval, InteractionResolution::UserInput { .. })
        | (InteractionKind::ToolApproval, InteractionResolution::PlanConfirmation { .. })
        | (InteractionKind::PlanConfirmation, InteractionResolution::UserInput { .. })
        | (InteractionKind::PlanConfirmation, InteractionResolution::ToolApproval { .. }) => false,
    }
}

pub async fn persist_and_emit(
    studio: &StudioRuntime,
    app: &AppHandle,
    interaction: &InteractionRequest,
) -> Result<()> {
    studio.store().upsert_interaction(interaction).await?;
    let payload = InteractionChangedPayload {
        session_id: interaction.scope.session_id.clone(),
        event: InteractionChangedEvent {
            interaction: interaction.clone(),
        },
    };
    let _ = app.emit("studio-interaction-changed", payload);
    Ok(())
}

fn cancelled_resolution(kind: &InteractionKind, reason: &str) -> InteractionResolution {
    match kind {
        InteractionKind::UserInput => InteractionResolution::UserInput {
            answers: Default::default(),
        },
        InteractionKind::ToolApproval => InteractionResolution::ToolApproval {
            decision: ToolApprovalResolution::Denied,
            reason: Some(reason.to_string()),
        },
        InteractionKind::PlanConfirmation => InteractionResolution::PlanConfirmation {
            decision: PlanConfirmationResolution::Dismiss,
            content: None,
            reason: Some(reason.to_string()),
        },
    }
}

fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
