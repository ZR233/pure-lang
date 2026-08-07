mod handoff;
mod lifecycle;
mod metadata;
mod tool;

pub(crate) use handoff::{
    TASK_EXECUTOR_HANDOFF_SECTION_ID, TaskExecutorDependencyV1, TaskExecutorEvidenceV1,
    TaskExecutorHandoffInput, TaskExecutorHandoffV1, TaskExecutorVerificationCommandV1,
};
pub(crate) use lifecycle::{
    StudioTaskSpawnPreparation, StudioTaskSpawnRequest, normalize_scope_hints,
};
pub(crate) use metadata::{StudioSpawnIntent, StudioTaskExecutorIntent};
