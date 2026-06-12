use pl_protocol::AgentEvent;

use crate::agent::AgentRecord;

pub(crate) fn emit_agent_record(event_tx: &pl_protocol::AgentEventSender, record: &AgentRecord) {
    let _ = event_tx.send(AgentEvent::AgentStateChanged {
        id: record.id.clone(),
        path: record.path.clone(),
        parent_path: record.parent_path.clone(),
        role: record.role.clone(),
        task: record.task.clone(),
        status: record.status,
        summary: record.summary.clone(),
        depth: record.depth,
        error: record.error.clone(),
        reason: record.reason.clone(),
        budget_limit_kind: record.budget_limit_kind,
        budget_usage: record.budget_usage,
        updated_at: record.updated_at,
    });
}

pub(super) async fn forward_agent_lifecycle_events(
    mut event_rx: tokio::sync::broadcast::Receiver<AgentEvent>,
    parent_event_tx: pl_protocol::AgentEventSender,
) {
    loop {
        match event_rx.recv().await {
            Ok(
                event @ (AgentEvent::AgentStateChanged { .. }
                | AgentEvent::AgentRuntimeUpdated { .. }
                | AgentEvent::CollabAgentSpawnBegin { .. }
                | AgentEvent::CollabAgentSpawnEnd { .. }
                | AgentEvent::CollabAgentInteractionBegin { .. }
                | AgentEvent::CollabAgentInteractionEnd { .. }
                | AgentEvent::CollabWaitingBegin { .. }
                | AgentEvent::CollabWaitingEnd { .. }
                | AgentEvent::CollabCloseBegin { .. }
                | AgentEvent::CollabCloseEnd { .. }
                | AgentEvent::InteractionChanged { .. }),
            ) => {
                let _ = parent_event_tx.send(event);
            }
            Ok(AgentEvent::Done) => break,
            Ok(
                AgentEvent::TimelineItemStarted { .. }
                | AgentEvent::TimelineItemDelta { .. }
                | AgentEvent::TimelineItemCompleted { .. }
                | AgentEvent::TimelineItemFailed { .. }
                | AgentEvent::TurnInterrupted { .. }
                | AgentEvent::TurnBudgetLimited { .. }
                | AgentEvent::Error { .. },
            ) => {}
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        }
    }
}
