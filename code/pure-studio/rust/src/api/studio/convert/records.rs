use crate::api::studio::types::ProjectDto;

pub(crate) fn project_dto(project: pl_studio_runtime::ProjectRecord) -> ProjectDto {
    ProjectDto {
        id: project.id,
        name: project.name,
        path: project.path,
        updated_at: project.updated_at,
    }
}

pub(crate) fn thread_from_record(value: pl_studio_runtime::ThreadRecord) -> pl_protocol::Thread {
    value.into()
}
