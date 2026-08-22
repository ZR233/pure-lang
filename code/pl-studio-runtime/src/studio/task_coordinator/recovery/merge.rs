use super::super::TaskRun;

pub(crate) const MERGE_RECOVERY_BLOCK_PREFIX: &str =
    "merge recovery requires planner reconciliation:";
const LEGACY_MERGE_RECOVERY_BLOCK_MESSAGE: &str = "planner Git integration was interrupted before task_record_merge; preserving the workspace for manual accounting";

pub(crate) enum MergingRecovery {
    Resume,
}

pub(crate) fn is_retryable_merge_recovery_message(message: &str) -> bool {
    message.starts_with(MERGE_RECOVERY_BLOCK_PREFIX)
        || message == LEGACY_MERGE_RECOVERY_BLOCK_MESSAGE
}

pub(crate) async fn inspect_merging_recovery(_run: &TaskRun) -> MergingRecovery {
    MergingRecovery::Resume
}
