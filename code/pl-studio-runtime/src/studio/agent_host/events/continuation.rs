use pl_core::{
    AgentRuntimeHandle, AgentSubmitRequest, AgentTurnSubmitPolicy, MailboxBudgetAction,
    MailboxPresentation, ThreadId,
};

use crate::studio::StudioStore;
use crate::studio::task_coordinator::ExecutorContinuationRequest;

pub(super) async fn submit_executor_continuation(
    runtime: &AgentRuntimeHandle,
    continuation: &ExecutorContinuationRequest,
) -> anyhow::Result<()> {
    let agent_id = ThreadId::new(continuation.agent_id.clone())?;
    let thread_id = ThreadId::new(continuation.agent_id.clone())?;
    let request = AgentSubmitRequest::start(
        thread_id,
        "Continue the assigned task from the compacted canonical session. Re-read current task status, finish the remaining work, verify it, and report completion.",
    )
    .with_presentation(MailboxPresentation::Hidden)
    .with_metadata(serde_json::json!({
        "kind": "executorBudgetContinuation",
        "workUnitId": continuation.work_unit_id,
        "sourceTurnId": continuation.source_turn_id,
        "slice": continuation.slice_count,
    }))
    .with_mail_id(continuation.mail_id())
    .with_turn_policy(AgentTurnSubmitPolicy::StartOnly);
    runtime
        .submit(agent_id, request)
        .await
        .map(|_| ())
        .map_err(|error| anyhow::anyhow!(error.to_string()))
}

pub(super) async fn recover_executor_continuation(
    runtime: &AgentRuntimeHandle,
    store: &StudioStore,
    continuation: &ExecutorContinuationRequest,
) -> anyhow::Result<()> {
    if let Some(turn_id) = store.executor_continuation_turn_id(continuation).await? {
        let agent_id = ThreadId::new(continuation.agent_id.clone())?;
        let snapshot = runtime.snapshot(agent_id).await?;
        if snapshot
            .active_turn_id()
            .is_some_and(|active| active.as_str() == turn_id)
        {
            store
                .mark_executor_turn_started(
                    &continuation.agent_id,
                    &turn_id,
                    MailboxBudgetAction::Preserve,
                )
                .await?;
            return Ok(());
        }
        if let Some(outcome) = snapshot
            .last_turn
            .as_ref()
            .filter(|outcome| outcome.turn_id.as_str() == turn_id)
        {
            if let Some(next) = store
                .settle_executor_turn_finished(&continuation.agent_id, outcome)
                .await?
            {
                submit_executor_continuation(runtime, &next).await?;
            }
            return Ok(());
        }
    }
    submit_executor_continuation(runtime, continuation).await
}
