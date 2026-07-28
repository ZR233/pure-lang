use crate::api::studio::types::{ProjectDto, SessionDto};
use pl_studio_runtime::SessionRecord;
// ── Project/Session DTOs ──

pub(crate) fn project_dto(project: pl_studio_runtime::ProjectRecord) -> ProjectDto {
    ProjectDto {
        id: project.id,
        name: project.name,
        path: project.path,
        updated_at: project.updated_at,
    }
}

pub(crate) fn session_dto(session: SessionRecord) -> SessionDto {
    SessionDto {
        id: session.id,
        project_id: session.project_id,
        title: session.title,
        mode: session.mode,
        created_at: session.created_at,
        updated_at: session.updated_at,
        visibility: session.visibility.as_str().to_string(),
        parent_session_id: session.parent_session_id,
        root_session_id: session.root_session_id,
        session_kind: session.session_kind.as_str().to_string(),
        owner_agent_id: session.owner_agent_id,
        owner_role: session.owner_role,
        agent_status: session.agent_status,
        agent_summary: session.agent_summary,
        agent_error: session.agent_error,
        agent_updated_at: session.agent_updated_at,
    }
}

pub(crate) fn session_summary_dto(session: pl_studio_runtime::StudioSessionSummary) -> SessionDto {
    SessionDto {
        id: session.id,
        project_id: session.project_id,
        title: session.title,
        mode: session.mode,
        created_at: session.created_at,
        updated_at: session.updated_at,
        visibility: session.visibility,
        parent_session_id: session.parent_session_id,
        root_session_id: session.root_session_id,
        session_kind: session.session_kind,
        owner_agent_id: session.owner_agent_id,
        owner_role: session.owner_role,
        agent_status: session.agent_status,
        agent_summary: session.agent_summary,
        agent_error: session.agent_error,
        agent_updated_at: session.agent_updated_at,
    }
}
