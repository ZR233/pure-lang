use anyhow::{Context, Result};
use pl_core::{
    BuiltinMcpServerState, CompileMode, McpServerConfig, McpServerTransport, ModelRole,
    PermissionMode, StudioSubmitPromptOptions, StudioSubmitPromptRequest, is_builtin_mcp_server_id,
};
use pl_protocol::{InteractionResolution, StudioEventEnvelope};

use crate::frb_generated::StreamSink;

use super::convert::{
    agent_bridge_dto, agent_event_bridge_dto, bridge_event_envelope, bridge_event_payload,
    bridge_interaction_changed, bridge_lsp_health, bridge_mcp_health, bridge_message, bridge_part,
    bridge_session_runtime_view, bridge_turn, interaction_request_bridge_dto,
    is_session_state_event, mcp_transport_from_label, normalized_string_list, project_dto,
    provider_settings_edit, provider_usage_dto, resolve_interaction_response, runtime_snapshot,
    session_dto, session_summary_dto,
};
use super::runtime::bridge;
use super::types::{
    BridgeEventEnvelope, BridgeSessionStateResponse, BridgeStudioEventsResponse,
    BridgeStudioMessageProjectionDto, BridgeStudioPartProjectionDto, BridgeStudioSnapshotResponse,
    ConfigSavedResponse, InstructionsSettingsInput, McpServerInput, McpSettingsInput,
    ProviderSettingsInput, ProviderUsagesResponse, ResolveInteractionResponse, RuntimeSnapshot,
    SkillSummaryDto, SkillsResponse, SkillsSettingsInput, StopPromptResponse, SubmitPromptResponse,
};

// ── Runtime lifecycle ──

pub fn initialize_runtime() -> Result<RuntimeSnapshot> {
    let bridge = bridge()?;
    bridge.block_on(async {
        bridge
            .studio
            .initialize_runtime()
            .await
            .map(runtime_snapshot)
    })
}

pub fn start_runtime() -> Result<RuntimeSnapshot> {
    let bridge = bridge()?;
    bridge.block_on(async { bridge.studio.start_runtime().await.map(runtime_snapshot) })
}

pub fn shutdown_runtime() -> Result<RuntimeSnapshot> {
    let bridge = bridge()?;
    bridge.block_on(async { bridge.studio.shutdown_runtime().await.map(runtime_snapshot) })
}

// ── Studio bootstrap ──

pub fn bootstrap_studio() -> Result<BridgeStudioSnapshotResponse> {
    let bridge = bridge()?;
    bridge.block_on(async { bootstrap_studio_inner(bridge).await })
}

pub fn open_project(path: String) -> Result<BridgeStudioSnapshotResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let project = bridge.studio.open_project(path).await?;
        bridge
            .studio
            .reconcile_lsp_runtime_for_project(&project.id)
            .await?;
        let _ = bridge.studio.ensure_project_sessions(&project.id).await?;
        studio_snapshot_inner(bridge, Some(project.id), None).await
    })
}

pub fn select_project(project_id: String) -> Result<BridgeStudioSnapshotResponse> {
    let bridge = bridge()?;
    bridge.block_on(async { studio_snapshot_inner(bridge, Some(project_id), None).await })
}

pub fn archive_project(
    project_id: String,
    selected_project_id: Option<String>,
) -> Result<BridgeStudioSnapshotResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        bridge
            .studio
            .archive_project(&project_id)
            .await?
            .context("selected project not found")?;
        let projects = bridge.studio.list_projects().await?;
        let next_project_id = selected_project_id
            .filter(|id| id != &project_id && projects.iter().any(|project| project.id == *id))
            .or_else(|| projects.first().map(|project| project.id.clone()));
        studio_snapshot_from_projects_inner(bridge, projects, next_project_id, None).await
    })
}

// ── Session management ──

pub fn create_session(project_id: String, title: String) -> Result<BridgeStudioSnapshotResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
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

// ── Settings ──

pub fn save_runtime_permission_mode(mode: String) -> Result<ConfigSavedResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let mut config = bridge.studio.config_store().load_or_default()?;
        config.runtime.permission_mode = PermissionMode::from_label(&mode);
        bridge.studio.config_store().save(&config)?;
        Ok(ConfigSavedResponse { saved: true })
    })
}

pub fn save_provider_settings(settings_json: String) -> Result<BridgeStudioSnapshotResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let input: ProviderSettingsInput =
            serde_json::from_str(&settings_json).context("invalid provider settings json")?;
        let current = bridge.studio.config_store().load_or_default()?;
        let next = provider_settings_edit(input, &current)?.to_config(&current)?;
        bridge.studio.config_store().save(&next)?;
        studio_snapshot_inner(bridge, None, None).await
    })
}

pub fn save_instructions_settings(settings_json: String) -> Result<BridgeStudioSnapshotResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let input: InstructionsSettingsInput =
            serde_json::from_str(&settings_json).context("invalid instructions settings json")?;
        let mut config = bridge.studio.config_store().load_or_default()?;
        config.instructions.base_override = input.base_override;
        config.instructions.developer = input.developer;
        config.instructions.user = input.user;
        config.instructions.project_doc_max_bytes = input.project_doc_max_bytes;
        config.instructions.project_doc_fallback_filenames =
            normalized_string_list(input.project_doc_fallback_filenames);
        config.validate()?;
        bridge.studio.config_store().save(&config)?;
        studio_snapshot_inner(bridge, None, None).await
    })
}

pub fn save_skills_settings(settings_json: String) -> Result<BridgeStudioSnapshotResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let input: SkillsSettingsInput =
            serde_json::from_str(&settings_json).context("invalid skills settings json")?;
        let mut config = bridge.studio.config_store().load_or_default()?;
        config.skills.enabled = input.enabled;
        config.skills.auto_learn = input.auto_learn;
        config.skills.system_enabled = input.system_enabled;
        config.skills.project_dir = input.project_dir;
        config.skills.user_dir = input.user_dir;
        config.skills.external_dirs = input.external_dirs;
        config.skills.disabled = input.disabled;
        config.skills.auto_learn_min_tool_calls = input.auto_learn_min_tool_calls;
        config.validate()?;
        bridge.studio.config_store().save(&config)?;
        studio_snapshot_inner(bridge, None, None).await
    })
}

pub fn save_mcp_settings(settings_json: String) -> Result<BridgeStudioSnapshotResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let input: McpSettingsInput =
            serde_json::from_str(&settings_json).context("invalid mcp settings json")?;
        let mut config = bridge.studio.config_store().load_or_default()?;
        let mut next_servers = std::mem::take(&mut config.mcp_servers);
        let mut next_builtin = std::mem::take(&mut config.builtin_mcp_servers);
        for server in input.servers {
            let server_id = server.id.trim().to_string();
            if server_id.is_empty() {
                continue;
            }
            if is_builtin_mcp_server_id(&server_id) {
                next_builtin.insert(
                    server_id,
                    BuiltinMcpServerState {
                        enabled: server.enabled,
                    },
                );
                continue;
            }
            let mut mcp_config =
                next_servers
                    .remove(&server_id)
                    .unwrap_or_else(|| McpServerConfig {
                        transport: mcp_transport_from_label(&server.transport),
                        ..Default::default()
                    });
            mcp_config.enabled = server.enabled;
            if !server.transport.trim().is_empty() {
                mcp_config.transport = mcp_transport_from_label(&server.transport);
            }
            let endpoint = server.endpoint.trim();
            match mcp_config.transport {
                McpServerTransport::Stdio => {
                    mcp_config.command = (!endpoint.is_empty()).then(|| endpoint.to_string());
                }
                McpServerTransport::StreamableHttp => {
                    mcp_config.url = (!endpoint.is_empty()).then(|| endpoint.to_string());
                }
            }
            next_servers.insert(server_id, mcp_config);
        }
        config.mcp_servers = next_servers;
        config.builtin_mcp_servers = next_builtin;
        config.validate()?;
        bridge.studio.config_store().save(&config)?;
        studio_snapshot_inner(bridge, None, None).await
    })
}

pub fn save_general_settings(settings_json: String) -> Result<BridgeStudioSnapshotResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let draft: serde_json::Value =
            serde_json::from_str(&settings_json).context("invalid general settings json")?;
        let normalized = serde_json::to_string(&draft)?;
        bridge
            .studio
            .store()
            .save_setting("flutterSettings:general", &normalized)
            .await?;
        studio_snapshot_inner(bridge, None, None).await
    })
}

// ── Provider usage ──

pub fn load_provider_usages() -> Result<ProviderUsagesResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let usages = bridge
            .studio
            .provider_usages()
            .await?
            .into_iter()
            .map(provider_usage_dto)
            .collect();
        Ok(ProviderUsagesResponse { usages })
    })
}

pub fn list_discovered_skills(project_id: String) -> Result<SkillsResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let catalog = bridge.studio.discovered_skills(&project_id).await?;
        Ok(SkillsResponse {
            skills: catalog
                .skills
                .into_iter()
                .map(|skill| SkillSummaryDto { name: skill.name })
                .collect(),
        })
    })
}

// ── Prompt / Interaction ──

pub fn submit_prompt(
    session_id: String,
    prompt: String,
    attachment_ids: Vec<String>,
) -> Result<SubmitPromptResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let response = bridge
            .studio
            .submit_prompt(StudioSubmitPromptRequest {
                session_id,
                prompt,
                attachment_ids,
                options: StudioSubmitPromptOptions::default(),
            })
            .await?;
        Ok(SubmitPromptResponse {
            session_id: response.session_id,
            turn_id: response.turn_id,
            cursor: response.cursor,
        })
    })
}

pub fn stop_prompt(session_id: String) -> Result<StopPromptResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let response = bridge.studio.stop_prompt(session_id).await?;
        Ok(StopPromptResponse {
            session_id: response.session_id,
            stopped: response.stopped,
        })
    })
}

pub fn resolve_interaction(
    interaction_id: String,
    resolution_json: String,
) -> Result<ResolveInteractionResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let resolution: InteractionResolution = serde_json::from_str(&resolution_json)
            .context("invalid interaction resolution json")?;
        let response = bridge
            .studio
            .resolve_interaction(interaction_id, resolution)
            .await?;
        Ok(resolve_interaction_response(response))
    })
}

// ── Session state / events ──

pub fn load_session_state(session_id: String) -> Result<BridgeSessionStateResponse> {
    let bridge = bridge()?;
    bridge.block_on(async { load_session_state_inner(bridge, session_id).await })
}

pub fn load_studio_events(
    session_id: String,
    after_sequence: Option<i64>,
    limit: Option<i64>,
) -> Result<BridgeStudioEventsResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let events = bridge
            .studio
            .store()
            .load_studio_events(&session_id, after_sequence, limit)
            .await?;
        let events = events
            .into_iter()
            .filter_map(bridge_event_envelope)
            .collect::<Vec<_>>();
        let next_sequence = bridge
            .studio
            .store()
            .next_studio_event_sequence(&session_id)
            .await? as u64;
        Ok(BridgeStudioEventsResponse {
            session_id,
            events,
            next_sequence,
        })
    })
}

pub fn subscribe_session_events(
    session_id: String,
    sink: StreamSink<BridgeEventEnvelope>,
) -> Result<()> {
    let bridge = bridge()?;
    let stale_session_id = session_id.clone();
    let mut events = bridge.studio.events().subscribe_session(session_id);
    bridge.tokio.spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => {
                    let Some(event) = bridge_event_envelope(event) else {
                        continue;
                    };
                    if sink.add(event).is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(lagged_events)) => {
                    if sink
                        .add(BridgeEventEnvelope::stale(
                            Some(stale_session_id.clone()),
                            lagged_events,
                        ))
                        .is_err()
                    {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
    Ok(())
}

pub fn subscribe_global_events(sink: StreamSink<BridgeEventEnvelope>) -> Result<()> {
    let bridge = bridge()?;
    let mut events = bridge.studio.events().subscribe_global();
    bridge.tokio.spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => {
                    let Some(event) = bridge_event_envelope(event) else {
                        continue;
                    };
                    if sink.add(event).is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(lagged_events)) => {
                    if sink
                        .add(BridgeEventEnvelope::stale(None, lagged_events))
                        .is_err()
                    {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
    Ok(())
}

// ── Inner async helpers ──

async fn bootstrap_studio_inner(
    bridge: &'static super::runtime::BridgeRuntime,
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

async fn studio_snapshot_inner(
    bridge: &'static super::runtime::BridgeRuntime,
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

async fn studio_snapshot_from_projects_inner(
    bridge: &'static super::runtime::BridgeRuntime,
    projects: Vec<pl_core::ProjectRecord>,
    requested_project_id: Option<String>,
    requested_session_id: Option<String>,
) -> Result<BridgeStudioSnapshotResponse> {
    let selected_project = requested_project_id
        .as_ref()
        .and_then(|project_id| {
            projects
                .iter()
                .find(|project| project.id == project_id)
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
    let config_json = serde_json::to_string(&bridge.studio.config_store().load_or_default()?)?;
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

async fn load_session_state_inner(
    bridge: &'static super::runtime::BridgeRuntime,
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
        session: super::convert::session_dto(session),
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
