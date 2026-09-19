use anyhow::{Context, Result};

use crate::studio::records::DirectoryState;

use crate::studio::entity as entities;
use crate::studio::records::{ProjectRecord, ThreadKind, ThreadRecord, ThreadVisibility};

pub fn project_record(model: entities::project::Model) -> ProjectRecord {
    ProjectRecord {
        id: model.id,
        name: model.name,
        path: model.path,
        ssh_alias: model.ssh_alias,
        updated_at: model.updated_at,
    }
}

pub fn thread_record(model: entities::thread::Model) -> Result<ThreadRecord> {
    let mode = pl_protocol::ThreadModeId::from_label(&model.mode)
        .map_err(|error| anyhow::anyhow!(error.to_string()))
        .with_context(|| format!("unsupported Thread mode in studio db: {}", model.id))?;
    let workspace_mode = pl_protocol::ThreadWorkspaceMode::from_label(&model.workspace_mode)
        .map_err(|error| anyhow::anyhow!(error.to_string()))
        .with_context(|| {
            format!(
                "unsupported Thread workspace mode in studio db: {}",
                model.id
            )
        })?;
    let state: DirectoryState = serde_json::from_str(&model.state_json)
        .with_context(|| format!("invalid Thread directory state: {}", model.id))?;
    let status = state.kind;
    let error = state.error;
    Ok(ThreadRecord {
        id: model.id.clone(),
        project_id: model.project_id,
        title: model.title,
        mode,
        workspace_mode,
        workspace_path: model.workspace_path,
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
