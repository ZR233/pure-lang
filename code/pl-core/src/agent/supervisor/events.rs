use pl_protocol::SubAgentActivityKind;
use pl_trace::AgentEvent;

use super::AgentRecord;

pub(crate) fn emit_agent_record(event_tx: &pl_trace::AgentEventSender, record: &AgentRecord) {
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_subagent_activity(
    event_tx: &pl_trace::AgentEventSender,
    call_id: String,
    agent: Option<&AgentRecord>,
    kind: SubAgentActivityKind,
    message: Option<String>,
    timed_out: Option<bool>,
    error: Option<String>,
) {
    let _ = event_tx.send(AgentEvent::SubAgentActivity {
        call_id,
        occurred_at: super::snapshot::unix_seconds(),
        agent_id: agent.map(|agent| agent.id.clone()),
        path: agent.map(|agent| agent.path.clone()),
        parent_path: agent.and_then(|agent| agent.parent_path.clone()),
        kind,
        status: agent.map(|agent| agent.status),
        message,
        timed_out,
        error,
    });
}

pub(super) async fn forward_agent_lifecycle_events(
    mut event_rx: tokio::sync::broadcast::Receiver<AgentEvent>,
    parent_event_tx: pl_trace::AgentEventSender,
) {
    loop {
        match event_rx.recv().await {
            Ok(
                event @ (AgentEvent::AgentStateChanged { .. }
                | AgentEvent::AgentRuntimeUpdated { .. }
                | AgentEvent::SubAgentActivity { .. }
                | AgentEvent::TodoListUpdated { .. }
                | AgentEvent::InteractionChanged { .. }
                | AgentEvent::SkillActivated { .. }),
            ) => {
                let _ = parent_event_tx.send(event);
            }
            Ok(AgentEvent::Done) => break,
            Ok(
                AgentEvent::TracePartStarted { .. }
                | AgentEvent::TracePartDelta { .. }
                | AgentEvent::TracePartCompleted { .. }
                | AgentEvent::TracePartFailed { .. }
                | AgentEvent::TurnInterrupted { .. }
                | AgentEvent::TurnBudgetLimited { .. }
                | AgentEvent::Error { .. },
            ) => {}
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        }
    }
}
