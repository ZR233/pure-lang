use anyhow::{Context, Result};

use pl_core::AgentState;

use crate::studio::entity as entities;
use crate::studio::records::{
    AttachmentRecord, ProjectRecord, ThreadKind, ThreadRecord, ThreadVisibility,
};
use crate::{InteractionContent, InteractionPurpose, InteractionRequest, InteractionScope};

pub fn project_record(model: entities::project::Model) -> ProjectRecord {
    ProjectRecord {
        id: model.id,
        name: model.name,
        path: model.path,
        ssh_server_id: model.ssh_server_id,
        updated_at: model.updated_at,
    }
}

pub fn thread_record(model: entities::thread::Model) -> Result<ThreadRecord> {
    let mode = pl_protocol::ThreadModeId::from_label(&model.mode)
        .map_err(|error| anyhow::anyhow!(error.to_string()))
        .with_context(|| format!("unsupported Thread mode in studio db: {}", model.id))?;
    let state: AgentState = serde_json::from_str(&model.state_json)
        .with_context(|| format!("invalid Agent state in studio db: {}", model.id))?;
    let status = match &state {
        AgentState::Idle(_) => pl_protocol::ThreadStatus::Idle,
        AgentState::Queued(_) => pl_protocol::ThreadStatus::Queued,
        AgentState::Running(_) => pl_protocol::ThreadStatus::Running,
        AgentState::WaitingTool(_) => pl_protocol::ThreadStatus::WaitingTool,
        AgentState::WaitingInteraction(_) => pl_protocol::ThreadStatus::WaitingInteraction,
        AgentState::Cancelling(_) => pl_protocol::ThreadStatus::Cancelling,
        AgentState::Closing(_) => pl_protocol::ThreadStatus::Closing,
        AgentState::Closed(_) => pl_protocol::ThreadStatus::Closed,
        AgentState::Faulted(_) => pl_protocol::ThreadStatus::Faulted,
    };
    let error = match &state {
        AgentState::Faulted(state) => Some(state.error().message.clone()),
        AgentState::Idle(_)
        | AgentState::Queued(_)
        | AgentState::Running(_)
        | AgentState::WaitingTool(_)
        | AgentState::WaitingInteraction(_)
        | AgentState::Cancelling(_)
        | AgentState::Closing(_)
        | AgentState::Closed(_) => None,
    };
    Ok(ThreadRecord {
        id: model.id.clone(),
        project_id: model.project_id,
        title: model.title,
        mode,
        created_at: model.created_at,
        updated_at: model.updated_at,
        visibility: if model.archived == 0 {
            ThreadVisibility::Active
        } else {
            ThreadVisibility::Archived
        },
        parent_thread_id: model.parent_thread_id.clone(),
        root_thread_id: model.root_thread_id,
        thread_kind: if model.parent_thread_id.is_some() {
            ThreadKind::Agent
        } else {
            ThreadKind::Root
        },
        agent_path: model.id,
        role: model.role,
        status,
        summary: None,
        error,
        runtime_updated_at: Some(model.updated_at),
    })
}

pub fn attachment_record(model: entities::attachment::Model) -> Result<AttachmentRecord> {
    let modality = match model.kind.as_str() {
        "image" => pl_protocol::studio::StudioAttachmentModality::Image,
        "video" => pl_protocol::studio::StudioAttachmentModality::Video,
        "file" => pl_protocol::studio::StudioAttachmentModality::File,
        other => anyhow::bail!("invalid attachment kind in studio db: {other}"),
    };
    Ok(AttachmentRecord {
        id: model.id,
        thread_id: model.thread_id,
        modality,
        media_type: model.media_type,
        filename: model.filename,
        storage_path: model.storage_path,
        byte_size: model.byte_size.max(0) as u64,
        content_sha256: model.content_sha256,
        width: model.width.and_then(|value| u32::try_from(value).ok()),
        height: model.height.and_then(|value| u32::try_from(value).ok()),
        created_at: model.created_at,
    })
}

pub fn interaction_record(model: entities::interaction::Model) -> Result<InteractionRequest> {
    let content: InteractionContent = serde_json::from_str(&model.state_json)
        .with_context(|| format!("invalid Interaction state in studio db: {}", model.id))?;
    let purpose: InteractionPurpose = serde_json::from_str(&model.purpose_json)
        .with_context(|| format!("invalid Interaction purpose in studio db: {}", model.id))?;
    let continuation = serde_json::from_str(&model.continuation_json).with_context(|| {
        format!(
            "invalid Interaction continuation in studio db: {}",
            model.id
        )
    })?;
    let interaction = InteractionRequest {
        interaction_id: model.id,
        scope: InteractionScope {
            thread_id: model.thread_id,
            turn_id: model.turn_id,
            item_id: model.item_id,
            tool_id: model.tool_id,
            agent_path: model.agent_path,
            purpose,
        },
        revision: u64::try_from(model.revision)?,
        content,
        continuation,
        created_at: model.created_at,
        updated_at: model.updated_at,
    };
    anyhow::ensure!(
        interaction.kind().as_str() == model.interaction_kind,
        "stored Interaction kind discriminator mismatch"
    );
    anyhow::ensure!(
        interaction.status().as_str() == model.state_kind,
        "stored Interaction state discriminator mismatch"
    );
    Ok(interaction)
}
