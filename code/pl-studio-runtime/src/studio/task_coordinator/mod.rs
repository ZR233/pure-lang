mod agent_projection;
mod completion;
mod coordinator;
mod design;
pub(crate) mod git;
mod merge;
mod recovery;
pub(crate) mod review;
mod review_round;
mod scope_hint;
mod spawn;
mod task_run;
#[cfg(test)]
pub(crate) mod test_support;
mod types;
mod work_completion;
mod work_unit;

pub(crate) use coordinator::*;
#[cfg(test)]
pub(crate) use recovery::MERGE_RECOVERY_BLOCK_PREFIX;
pub(crate) use recovery::is_retryable_merge_recovery_message;
pub(crate) use review_round::*;
pub(crate) use spawn::{
    OperationalTaskSpawnFailure, StudioSpawnIntent, StudioTaskSpawnPreparation,
    StudioTaskSpawnRequest, TASK_EXECUTOR_HANDOFF_SECTION_ID, TaskExecutorHandoff,
    TaskSpawnCompensation, TaskSpawnCompensationState, TaskSpawnFailure, TaskSpawnFailureCode,
    TaskSpawnFailurePhase, TaskSpawnResource,
};
pub(crate) use task_run::*;
pub(crate) use types::*;
pub(crate) use work_unit::*;

#[cfg(test)]
mod unit_tests;
