use std::sync::Arc;

use anyhow::Result;
use pl_core::{InteractionEmitter, InteractionRequest, StudioRuntime};
use pl_protocol::InteractionChangedEvent;
use tauri::{AppHandle, Emitter};

use crate::dto::InteractionChangedPayload;

pub fn interaction_emitter(
    studio: StudioRuntime,
    app: AppHandle,
    fallback_session_id: String,
) -> InteractionEmitter {
    Arc::new(move |interaction: InteractionRequest| {
        let studio = studio.clone();
        let app = app.clone();
        let fallback_session_id = fallback_session_id.clone();
        Box::pin(async move {
            emit_interaction_changed(&studio, &app, &fallback_session_id, &interaction).await
        })
    })
}

pub async fn emit_interaction_changed(
    _studio: &StudioRuntime,
    app: &AppHandle,
    fallback_session_id: &str,
    interaction: &InteractionRequest,
) -> Result<()> {
    let session_id = if interaction.scope.session_id.trim().is_empty() {
        fallback_session_id.to_string()
    } else {
        interaction.scope.session_id.clone()
    };
    let payload = InteractionChangedPayload {
        session_id: session_id.clone(),
        event: InteractionChangedEvent {
            interaction: interaction.clone(),
        },
    };
    let _ = app.emit("studio-interaction-changed", payload);
    Ok(())
}
