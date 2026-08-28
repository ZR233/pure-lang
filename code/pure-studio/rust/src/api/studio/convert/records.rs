use crate::api::studio::types::ProjectDto;

impl From<pl_studio_runtime::ProjectRecord> for ProjectDto {
    fn from(project: pl_studio_runtime::ProjectRecord) -> Self {
        Self {
            id: project.id,
            name: project.name,
            path: project.path,
            ssh_server_id: project.ssh_server_id,
            updated_at: project.updated_at,
        }
    }
}

pub(crate) fn thread_from_record(value: pl_studio_runtime::ThreadRecord) -> pl_protocol::Thread {
    value.into()
}
