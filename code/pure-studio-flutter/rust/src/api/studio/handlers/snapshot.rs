use crate::api::studio::convert::agent::{agent_bridge_dto, agent_event_bridge_dto};
use crate::api::studio::convert::event::{bridge_event_envelope, is_session_state_event};
use crate::api::studio::convert::interaction::interaction_request_bridge_dto;
use crate::api::studio::convert::message::{bridge_message, bridge_part};
use crate::api::studio::convert::records::{project_dto, session_dto};
use crate::api::studio::convert::runtime::bridge_session_runtime_view;
use crate::api::studio::convert::settings::studio_config_projection;
use crate::api::studio::runtime::BridgeRuntime;
use crate::api::studio::types::{
    BridgeSessionStateResponse, BridgeStudioMessageProjectionDto, BridgeStudioPartProjectionDto,
    BridgeStudioSnapshotResponse,
};
use anyhow::{Context, Result};
// ── Inner async helpers ──

pub(super) async fn bootstrap_studio_inner(
    bridge: &'static BridgeRuntime,
) -> Result<BridgeStudioSnapshotResponse> {
    let mut projects = bridge.studio.list_projects().await?;
    if projects.is_empty()
        && !bridge.studio.store().has_projects().await?
        && let Ok(cwd) = std::env::current_dir()
    {
        projects.push(bridge.studio.open_project(cwd).await?);
    }
    let selected_project_id = projects.first().map(|project| project.id.clone());
    studio_snapshot_from_projects_inner(bridge, projects, selected_project_id, None).await
}

pub(super) async fn studio_snapshot_inner(
    bridge: &'static BridgeRuntime,
    requested_project_id: Option<String>,
    requested_session_id: Option<String>,
) -> Result<BridgeStudioSnapshotResponse> {
    let projects = bridge.studio.list_projects().await?;
    studio_snapshot_from_projects_inner(
        bridge,
        projects,
        requested_project_id,
        requested_session_id,
    )
    .await
}

pub(super) async fn studio_snapshot_from_projects_inner(
    bridge: &'static BridgeRuntime,
    projects: Vec<pl_studio_runtime::ProjectRecord>,
    requested_project_id: Option<String>,
    requested_session_id: Option<String>,
) -> Result<BridgeStudioSnapshotResponse> {
    let selected_project = requested_project_id
        .as_ref()
        .and_then(|project_id| {
            projects
                .iter()
                .find(|project| project.id == *project_id)
                .cloned()
        })
        .or_else(|| projects.first().cloned());
    let selected_project_id = selected_project.as_ref().map(|project| project.id.clone());
    let mut sessions = Vec::new();
    let mut selected_session_id = None;
    let mut agent_events = Vec::new();
    let mut agents = Vec::new();
    let mut interactions = Vec::new();

    if let Some(project) = selected_project {
        bridge
            .studio
            .reconcile_lsp_runtime_for_project(&project.id)
            .await?;
        sessions = bridge.studio.ensure_project_sessions(&project.id).await?;
        selected_session_id = requested_session_id
            .filter(|session_id| sessions.iter().any(|session| session.id == *session_id))
            .or_else(|| sessions.first().map(|session| session.id.clone()));
        if let Some(session_id) = selected_session_id.as_deref() {
            agent_events = bridge.studio.store().list_agent_events(session_id).await?;
            agents = bridge.studio.store().list_agents(session_id).await?;
            interactions = bridge
                .studio
                .store()
                .list_pending_interactions(session_id)
                .await?;
        }
    }
    let session_runtime = match selected_session_id.as_deref() {
        Some(session_id) => Some(bridge_session_runtime_view(bridge, session_id).await?),
        None => None,
    };
    let config = bridge.studio.config_store().load_or_default()?;
    let config_json = serde_json::to_string(&studio_config_projection(&config)?)?;
    let general_settings = bridge
        .studio
        .store()
        .load_setting("flutterSettings:general")
        .await?
        .and_then(|value| serde_json::from_str::<serde_json::Value>(&value).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    let general_settings_json = serde_json::to_string(&general_settings)?;

    Ok(BridgeStudioSnapshotResponse {
        projects: projects.into_iter().map(project_dto).collect(),
        selected_project_id,
        sessions: sessions.into_iter().map(session_dto).collect(),
        selected_session_id,
        agent_events: agent_events
            .into_iter()
            .map(agent_event_bridge_dto)
            .collect::<Result<Vec<_>>>()?,
        agents: agents.into_iter().map(agent_bridge_dto).collect(),
        interactions: interactions
            .into_iter()
            .map(interaction_request_bridge_dto)
            .collect(),
        session_runtime,
        config_json,
        general_settings_json,
    })
}

pub(super) async fn load_session_state_inner(
    bridge: &'static BridgeRuntime,
    session_id: String,
) -> Result<BridgeSessionStateResponse> {
    let session = bridge
        .studio
        .store()
        .read_session(&session_id)
        .await?
        .context("selected session not found")?;
    let events = bridge
        .studio
        .store()
        .load_studio_events(&session_id, None, None)
        .await?
        .into_iter()
        .filter(is_session_state_event)
        .filter_map(bridge_event_envelope)
        .collect();
    let messages = bridge
        .studio
        .store()
        .load_studio_messages(&session_id)
        .await?
        .into_iter()
        .map(|record| BridgeStudioMessageProjectionDto {
            message: bridge_message(record.message),
            sequence: record.sequence.max(0) as u64,
        })
        .collect();
    let parts = bridge
        .studio
        .store()
        .load_message_parts(&session_id)
        .await?
        .into_iter()
        .map(|record| BridgeStudioPartProjectionDto {
            part: bridge_part(record.part),
            sequence: record.sequence.max(0) as u64,
        })
        .collect();
    let event_next_sequence = bridge
        .studio
        .store()
        .next_studio_event_sequence(&session_id)
        .await? as u64;
    let sessions = bridge
        .studio
        .store()
        .list_sessions(&session.project_id)
        .await?
        .into_iter()
        .map(session_dto)
        .collect();
    let agents = bridge
        .studio
        .store()
        .list_agents(&session_id)
        .await?
        .into_iter()
        .map(agent_bridge_dto)
        .collect();
    let agent_events = bridge
        .studio
        .store()
        .list_agent_events(&session_id)
        .await?
        .into_iter()
        .map(agent_event_bridge_dto)
        .collect::<Result<Vec<_>>>()?;
    let interactions = bridge
        .studio
        .store()
        .list_pending_interactions(&session_id)
        .await?
        .into_iter()
        .map(interaction_request_bridge_dto)
        .collect();
    let session_runtime = bridge_session_runtime_view(bridge, &session_id).await.ok();

    Ok(BridgeSessionStateResponse {
        session_id: session_id.clone(),
        session: session_dto(session),
        sessions,
        messages,
        parts,
        events,
        event_next_sequence,
        agents,
        agent_events,
        interactions,
        session_runtime,
    })
}
