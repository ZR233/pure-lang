use super::snapshot::{load_session_state_inner, studio_snapshot_inner};
use crate::api::studio::runtime::bridge;
use crate::api::studio::types::{BridgeSessionStateResponse, BridgeStudioSnapshotResponse};
use anyhow::{Context, Result};
use pl_core::{CompileMode, ModelRole};
// ── Session management ──

pub fn create_session(
    project_id: String,
    title: Option<String>,
) -> Result<BridgeStudioSnapshotResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let title = title.unwrap_or_else(|| "New Session".to_string());
        let session = bridge.studio.create_session(&project_id, &title).await?;
        studio_snapshot_inner(bridge, Some(project_id), Some(session.id)).await
    })
}

pub fn archive_session(
    session_id: String,
    selected_session_id: Option<String>,
) -> Result<BridgeStudioSnapshotResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let archived = bridge
            .studio
            .archive_session(session_id.clone())
            .await?
            .context("selected session not found")?;
        let sessions = bridge
            .studio
            .store()
            .list_sessions(&archived.project_id)
            .await?;
        let next_session_id = selected_session_id
            .filter(|id| id != &session_id && sessions.iter().any(|session| session.id == *id))
            .or_else(|| sessions.first().map(|session| session.id.clone()));
        studio_snapshot_inner(bridge, Some(archived.project_id), next_session_id).await
    })
}

pub fn set_session_mode(session_id: String, mode: String) -> Result<BridgeSessionStateResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        bridge
            .studio
            .set_session_mode(&session_id, CompileMode::from_label(&mode))
            .await?;
        load_session_state_inner(bridge, session_id).await
    })
}

// ── Model role ──

pub fn set_model_role(
    role_key: String,
    provider_id: String,
    model: String,
    effort: Option<String>,
    selected_session_id: Option<String>,
) -> Result<BridgeStudioSnapshotResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let role = ModelRole::from_key(role_key.trim())
            .with_context(|| format!("unsupported model role: {role_key}"))?;
        bridge
            .studio
            .set_model_role(role, &provider_id, &model, effort.as_deref())?;
        let selected_session_id = selected_session_id.filter(|value| !value.trim().is_empty());
        let selected_project_id = match selected_session_id.as_deref() {
            Some(session_id) => Some(
                bridge
                    .studio
                    .store()
                    .read_session(session_id)
                    .await?
                    .context("selected session not found")?
                    .project_id,
            ),
            None => None,
        };
        studio_snapshot_inner(bridge, selected_project_id, selected_session_id).await
    })
}

// ── Session state / events ──

pub fn load_session_state(session_id: String) -> Result<BridgeSessionStateResponse> {
    let bridge = bridge()?;
    bridge.block_on(async { load_session_state_inner(bridge, session_id).await })
}
