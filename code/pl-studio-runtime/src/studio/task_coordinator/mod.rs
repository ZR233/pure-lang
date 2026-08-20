mod agent_projection;
mod completion;
mod coordinator;
mod design;
pub(crate) mod git;
mod merge;
mod recovery;
pub(crate) mod review;
mod scope_hint;
mod spawn;
mod types;
mod work_completion;

pub(crate) use coordinator::*;
#[cfg(test)]
pub(crate) use recovery::MERGE_RECOVERY_BLOCK_PREFIX;
pub(crate) use recovery::is_retryable_merge_recovery_message;
pub(crate) use spawn::{
    StudioSpawnIntent, StudioTaskSpawnPreparation, StudioTaskSpawnRequest,
    TASK_EXECUTOR_HANDOFF_SECTION_ID, TaskExecutorHandoff,
};
pub(crate) use types::*;

#[cfg(test)]
mod tests;
