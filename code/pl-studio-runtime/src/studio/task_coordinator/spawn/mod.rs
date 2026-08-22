mod failure;
mod handoff;
mod lifecycle;
mod metadata;
mod tool;

pub(crate) use failure::{
    OperationalTaskSpawnFailure, TaskSpawnCompensation, TaskSpawnCompensationState,
    TaskSpawnFailure, TaskSpawnFailureCode, TaskSpawnFailurePhase, TaskSpawnResource,
};
pub(crate) use handoff::{
    TASK_EXECUTOR_HANDOFF_SECTION_ID, TaskExecutorAcceptanceCriterion, TaskExecutorBlueprint,
    TaskExecutorDependency, TaskExecutorEvidence, TaskExecutorHandoff,
    TaskExecutorImplementationStep, TaskExecutorScope, TaskExecutorVerificationContract,
    verification_result_map,
};
#[cfg(test)]
pub(crate) use handoff::{TaskExecutorTarget, TaskExecutorVerificationCommand};
pub(crate) use lifecycle::{
    StudioTaskSpawnPreparation, StudioTaskSpawnRequest, normalize_scope_hints,
};
pub(crate) use metadata::{StudioSpawnIntent, StudioTaskExecutorIntent};
