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
        updated_at: session.updated_at,
        visibility: session.visibility.as_str().to_string(),
        parent_session_id: session.parent_session_id,
    }
}

pub(crate) fn session_summary_dto(session: pl_studio_runtime::StudioSessionSummary) -> SessionDto {
    SessionDto {
        id: session.id,
        project_id: session.project_id,
        title: session.title,
        mode: session.mode,
        updated_at: session.updated_at,
        visibility: session.visibility,
        parent_session_id: session.parent_session_id,
    }
}
