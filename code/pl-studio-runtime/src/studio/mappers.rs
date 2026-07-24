use crate::{
    InteractionPayload, InteractionRequest, InteractionResolution, InteractionScope,
    InteractionStatus,
};
use anyhow::{Context, Result, bail};

use crate::studio::entities;
use crate::studio::records::{
    AttachmentRecord, ProjectRecord, SessionKind, SessionRecord, SessionVisibility,
};

pub fn project_record(model: entities::project::Model) -> ProjectRecord {
    ProjectRecord {
        id: model.id,
        name: model.name,
        path: model.path,
        updated_at: model.updated_at,
    }
}

pub fn session_record(model: entities::session::Model) -> SessionRecord {
    let instruction_snapshot = model
        .instruction_snapshot_json
        .as_deref()
        .and_then(|json| serde_json::from_str(json).ok());
    SessionRecord {
        id: model.id,
        project_id: model.project_id,
        title: model.title,
        mode: model.mode,
        created_at: model.created_at,
        updated_at: model.updated_at,
        visibility: session_visibility_from_label(&model.visibility),
        parent_session_id: model.parent_session_id,
        root_session_id: model.root_session_id,
        session_kind: session_kind_from_label(&model.session_kind),
        owner_agent_id: model.owner_agent_id,
        owner_role: model.owner_role,
        agent_status: model.agent_status,
        agent_summary: model.agent_summary,
        agent_error: model.agent_error,
        agent_updated_at: model.agent_updated_at,
        instruction_snapshot,
    }
}

fn session_kind_from_label(label: &str) -> SessionKind {
    match label {
        "agent" => SessionKind::Agent,
        _ => SessionKind::Root,
    }
}

fn session_visibility_from_label(label: &str) -> SessionVisibility {
    match label {
        "active" => SessionVisibility::Active,
        "handoffOrigin" => SessionVisibility::HandoffOrigin,
        "archived" => SessionVisibility::Archived,
        _ => SessionVisibility::Archived,
    }
}

pub fn attachment_record(model: entities::attachment::Model) -> AttachmentRecord {
    AttachmentRecord {
        id: model.id,
        session_id: model.session_id,
        message_id: model.message_id,
        media_type: model.media_type,
        filename: model.filename,
        storage_path: model.storage_path,
        byte_size: model.byte_size.max(0) as u64,
        width: model.width.and_then(|value| u32::try_from(value).ok()),
        height: model.height.and_then(|value| u32::try_from(value).ok()),
        created_at: model.created_at,
    }
}

pub fn interaction_record(model: entities::interaction::Model) -> Result<InteractionRequest> {
    let payload =
        serde_json::from_str::<InteractionPayload>(&model.payload_json).with_context(|| {
            let id = &model.id;
            format!("failed to parse interaction payload: {id}")
        })?;
    let resolution = model
        .resolution_json
        .as_deref()
        .map(serde_json::from_str::<InteractionResolution>)
        .transpose()
        .with_context(|| {
            let id = &model.id;
            format!("failed to parse interaction resolution: {id}")
        })?;
    Ok(InteractionRequest {
        interaction_id: model.id,
        kind: interaction_kind_from_label(&model.kind)?,
        status: interaction_status_from_label(&model.status)?,
        scope: InteractionScope {
            session_id: model.session_id,
            turn_id: model.turn_id,
            item_id: model.item_id,
            tool_id: model.tool_id,
            agent_path: model.agent_path,
        },
        payload,
        created_at: model.created_at,
        updated_at: model.updated_at,
        resolved_at: model.resolved_at,
        resolution,
    })
}

fn interaction_kind_from_label(label: &str) -> Result<crate::InteractionKind> {
    match label {
        "userInput" => Ok(crate::InteractionKind::UserInput),
        "toolApproval" => Ok(crate::InteractionKind::ToolApproval),
        "planConfirmation" => Ok(crate::InteractionKind::PlanConfirmation),
        other => bail!("unsupported interaction kind in studio db: {other}"),
    }
}

fn interaction_status_from_label(label: &str) -> Result<InteractionStatus> {
    match label {
        "pending" => Ok(InteractionStatus::Pending),
        "resolved" => Ok(InteractionStatus::Resolved),
        "cancelled" => Ok(InteractionStatus::Cancelled),
        "expired" => Ok(InteractionStatus::Expired),
        other => bail!("unsupported interaction status in studio db: {other}"),
    }
}
