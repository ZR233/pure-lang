use pl_core::{
    AgentRuntimeHandle, AgentSubmitRequest, AgentTurnSubmitPolicy, MailboxPresentation, ThreadId,
};

use crate::studio::StudioStore;
use crate::studio::task_coordinator::{TaskPlannerWakeRequest, TaskPlannerWakeSource};

pub(in crate::studio) async fn materialize_task_planner_wake(
    runtime: &AgentRuntimeHandle,
    store: &StudioStore,
    wake: &TaskPlannerWakeRequest,
) -> anyhow::Result<()> {
    if store.task_planner_wake_was_delivered(wake).await? {
        return Ok(());
    }
    let (message, metadata) = match &wake.source {
        TaskPlannerWakeSource::Review {
            review_round_id,
            scope,
        } => (
            format!(
                "Review round {review_round_id} completed. Read task_status and list_agents, then continue from the canonical Task phase."
            ),
            serde_json::json!({
                "kind": "taskReviewContinuation",
                "taskRunId": wake.task_run_id,
                "reviewRoundId": review_round_id,
                "scope": scope,
            }),
        ),
        TaskPlannerWakeSource::ExecutorTerminal {
            work_unit_id,
            executor_thread_id,
            source_turn_id,
        } => (
            format!(
                "Executor {executor_thread_id} ended Turn {source_turn_id} without a new completion. Read task_status and list_agents, inspect the canonical execution status, then reconcile WorkUnit {work_unit_id}."
            ),
            serde_json::json!({
                "kind": "taskExecutorTerminal",
                "taskRunId": wake.task_run_id,
                "workUnitId": work_unit_id,
                "executorThreadId": executor_thread_id,
                "sourceTurnId": source_turn_id,
            }),
        ),
    };
    let root_agent_id = crate::studio::agent_host::root_agent_id(&wake.root_thread_id);
    let thread_id = ThreadId::new(wake.root_thread_id.clone())?;
    runtime
        .submit(
            root_agent_id,
            AgentSubmitRequest::start(thread_id, message)
                .with_presentation(MailboxPresentation::Hidden)
                .with_metadata(metadata)
                .with_queue_coalescing_key(format!("studio.task_planner_wake:{}", wake.task_run_id))
                .with_mail_id(wake.mail_id())
                .with_turn_policy(AgentTurnSubmitPolicy::StartOrQueue),
        )
        .await
        .map(|_| ())
        .map_err(|error| anyhow::anyhow!(error.to_string()))
}

pub(in crate::studio) async fn materialize_pending_task_planner_wakes(
    runtime: &AgentRuntimeHandle,
    store: &StudioStore,
    root_thread_id: Option<&str>,
) -> anyhow::Result<()> {
    for wake in store.list_pending_task_planner_wakes().await? {
        if root_thread_id.is_some_and(|root| wake.root_thread_id != root) {
            continue;
        }
        let root_agent_id = crate::studio::agent_host::root_agent_id(&wake.root_thread_id);
        match runtime.snapshot(root_agent_id).await {
            Ok(_) => {}
            Err(pl_core::AgentRuntimeError::NotFound(_)) if root_thread_id.is_none() => continue,
            Err(error) => return Err(anyhow::anyhow!(error.to_string())),
        }
        materialize_task_planner_wake(runtime, store, &wake).await?;
    }
    Ok(())
}
