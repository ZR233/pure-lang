use anyhow::{Context, Result};

use pl_core::AgentState;

use crate::studio::entity as entities;
use crate::studio::records::{ProjectRecord, ThreadKind, ThreadRecord, ThreadVisibility};

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
    let mode = pl_core::ThreadModeId::from_label(&model.mode)
        .map_err(|error| anyhow::anyhow!(error.to_string()))
        .with_context(|| format!("unsupported Thread mode in studio db: {}", model.id))?;
    let state: AgentState = serde_json::from_str(&model.state_json)
        .with_context(|| format!("invalid Agent state in studio db: {}", model.id))?;
    let status = match &state {
        AgentState::Idle(_) => pl_core::ThreadStatus::Idle,
        AgentState::Queued(_) => pl_core::ThreadStatus::Queued,
        AgentState::Running(_) => pl_core::ThreadStatus::Running,
        AgentState::WaitingTool(_) => pl_core::ThreadStatus::WaitingTool,
        AgentState::WaitingInteraction(_) => pl_core::ThreadStatus::WaitingInteraction,
        AgentState::Cancelling(_) => pl_core::ThreadStatus::Cancelling,
        AgentState::Closing(_) => pl_core::ThreadStatus::Closing,
        AgentState::Closed(_) => pl_core::ThreadStatus::Closed,
        AgentState::Faulted(_) => pl_core::ThreadStatus::Faulted,
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
