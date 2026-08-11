use anyhow::{Context, Result};

use pl_protocol::LabeledEnum;

use crate::studio::entity as entities;
use crate::studio::records::{
    AttachmentRecord, ProjectRecord, ThreadKind, ThreadRecord, ThreadVisibility,
};
use crate::{
    InteractionKind, InteractionPayload, InteractionRequest, InteractionResolution,
    InteractionScope, InteractionStatus,
};

pub fn project_record(model: entities::project::Model) -> ProjectRecord {
    ProjectRecord {
        id: model.id,
        name: model.name,
        path: model.path,
        updated_at: model.updated_at,
    }
}

pub fn thread_record(model: entities::thread::Model) -> ThreadRecord {
    ThreadRecord {
        id: model.id.clone(),
        project_id: model.project_id,
        title: model.title,
        mode: model.mode,
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
        status: model.status,
        summary: None,
        error: None,
        runtime_updated_at: Some(model.updated_at),
    }
}

pub fn attachment_record(model: entities::attachment::Model) -> AttachmentRecord {
    AttachmentRecord {
        id: model.id,
        thread_id: model.thread_id,
        item_id: model.item_id,
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
    let kind = InteractionKind::from_label(&model.kind)
        .map_err(|error| anyhow::anyhow!(error.to_string()))
        .with_context(|| format!("unsupported interaction kind in studio db: {}", model.id))?;
    let status = InteractionStatus::from_label(&model.status)
        .map_err(|error| anyhow::anyhow!(error.to_string()))
        .with_context(|| format!("unsupported interaction status in studio db: {}", model.id))?;
    Ok(InteractionRequest {
        interaction_id: model.id,
        kind,
        status,
        scope: InteractionScope {
            thread_id: model.thread_id,
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
