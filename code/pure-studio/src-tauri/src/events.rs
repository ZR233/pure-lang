use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use pl_core::{StudioAgentSnapshotRecord, StudioAgentTimelineEventRecord, StudioStore};
use pl_protocol::AgentEvent;
use tauri::{AppHandle, Emitter};
use tokio::sync::broadcast::error::RecvError;

use crate::dto::AgentEventPayload;
use crate::mappers::{agent_dto, agent_event_dto};

static EVENT_COUNTER: AtomicU64 = AtomicU64::new(0);

pub async fn drain_events(
    session_id: String,
    mut event_rx: tokio::sync::broadcast::Receiver<AgentEvent>,
    app: AppHandle,
    store: StudioStore,
) {
    loop {
        match event_rx.recv().await {
            Ok(event) => {
                let done = matches!(event, AgentEvent::Done);
                let agent = if let Some(record) = agent_snapshot_record(&session_id, &event) {
                    let _ = store.upsert_agent_snapshot(record.clone()).await;
                    Some(agent_dto(record))
                } else {
                    None
                };
                let timeline_event = if let Ok(sequence) =
                    store.next_agent_event_sequence(&session_id).await
                {
                    if let Some(record) = agent_timeline_event_record(&session_id, sequence, &event)
                    {
                        let _ = store.record_agent_event(record.clone()).await;
                        Some(agent_event_dto(record))
                    } else {
                        None
                    }
                } else {
                    None
                };
                let event_for_legacy = if matches!(
                    event,
                    AgentEvent::TextDelta { .. }
                        | AgentEvent::ThinkingDelta { .. }
                        | AgentEvent::ToolCallDelta { .. }
                        | AgentEvent::ToolCallComplete { .. }
                        | AgentEvent::ToolApprovalRequested { .. }
                        | AgentEvent::ToolApprovalGranted { .. }
                        | AgentEvent::ToolApprovalDenied { .. }
                        | AgentEvent::TurnStarted
                        | AgentEvent::TurnInterrupted { .. }
                        | AgentEvent::TurnBudgetLimited { .. }
                        | AgentEvent::Done
                        | AgentEvent::Error { .. }
                ) {
                    Some(event)
                } else {
                    None
                };
                if event_for_legacy.is_some() || timeline_event.is_some() || agent.is_some() {
                    let _ = app.emit(
                        "studio-agent-event",
                        AgentEventPayload {
                            session_id: session_id.clone(),
                            event: event_for_legacy,
                            timeline_event,
                            agent,
                        },
                    );
                }
                if done {
                    break;
                }
            }
            Err(RecvError::Lagged(_)) => {
                continue;
            }
            Err(RecvError::Closed) => {
                break;
            }
        }
    }
}

pub fn agent_snapshot_record(
    session_id: &str,
    event: &AgentEvent,
) -> Option<StudioAgentSnapshotRecord> {
    match event {
        AgentEvent::AgentStateChanged {
            id,
            path,
            parent_path,
            role,
            task,
            status,
            summary,
            depth,
            error,
            reason,
            budget_limit_kind,
            budget_usage,
            updated_at,
        } => Some(StudioAgentSnapshotRecord {
            id: id.clone(),
            session_id: session_id.to_string(),
            path: path.clone(),
            parent_path: parent_path.clone(),
            role: role.clone(),
            task: task.clone(),
            status: *status,
            summary: summary.clone(),
            depth: *depth as i32,
            error: error.clone(),
            reason: reason.clone(),
            budget_limit_kind: *budget_limit_kind,
            budget_usage: *budget_usage,
            updated_at: *updated_at,
        }),
        _ => None,
    }
}

pub fn agent_timeline_event_record(
    session_id: &str,
    sequence: i64,
    event: &AgentEvent,
) -> Option<StudioAgentTimelineEventRecord> {
    let (kind, agent_id, path, parent_path) = match event {
        AgentEvent::AgentStateChanged {
            id,
            path,
            parent_path,
            ..
        } => (
            "agentStatus".to_string(),
            Some(id.clone()),
            Some(path.clone()),
            parent_path.clone(),
        ),
        AgentEvent::CollabAgentSpawnBegin { sender_path, .. } => (
            "spawnBegin".to_string(),
            None,
            Some(sender_path.clone()),
            None,
        ),
        AgentEvent::CollabAgentSpawnEnd { agent_id, path, .. } => {
            ("spawnEnd".to_string(), agent_id.clone(), path.clone(), None)
        }
        AgentEvent::CollabAgentInteractionBegin {
            receiver_path,
            sender_path,
            ..
        } => (
            "interactionBegin".to_string(),
            None,
            Some(receiver_path.clone()),
            Some(sender_path.clone()),
        ),
        AgentEvent::CollabAgentInteractionEnd {
            receiver_path,
            sender_path,
            ..
        } => (
            "interactionEnd".to_string(),
            None,
            Some(receiver_path.clone()),
            Some(sender_path.clone()),
        ),
        AgentEvent::CollabWaitingBegin { sender_path, .. } => (
            "waitingBegin".to_string(),
            None,
            Some(sender_path.clone()),
            None,
        ),
        AgentEvent::CollabWaitingEnd { sender_path, .. } => (
            "waitingEnd".to_string(),
            None,
            Some(sender_path.clone()),
            None,
        ),
        AgentEvent::CollabCloseBegin {
            receiver_path,
            sender_path,
            ..
        } => (
            "closeBegin".to_string(),
            None,
            Some(receiver_path.clone()),
            Some(sender_path.clone()),
        ),
        AgentEvent::CollabCloseEnd {
            receiver_path,
            sender_path,
            ..
        } => (
            "closeEnd".to_string(),
            None,
            Some(receiver_path.clone()),
            Some(sender_path.clone()),
        ),
        AgentEvent::TextDelta { .. }
        | AgentEvent::ThinkingDelta { .. }
        | AgentEvent::ToolCallDelta { .. }
        | AgentEvent::ToolCallComplete { .. }
        | AgentEvent::ToolApprovalRequested { .. }
        | AgentEvent::ToolApprovalGranted { .. }
        | AgentEvent::ToolApprovalDenied { .. }
        | AgentEvent::TurnStarted
        | AgentEvent::TurnInterrupted { .. }
        | AgentEvent::TurnBudgetLimited { .. }
        | AgentEvent::Done
        | AgentEvent::Error { .. } => return None,
    };
    let payload_json = serde_json::to_string(event).ok()?;
    Some(StudioAgentTimelineEventRecord {
        event_id: new_event_id("agent-event"),
        session_id: session_id.to_string(),
        sequence,
        kind,
        agent_id,
        path,
        parent_path,
        payload_json,
        created_at: unix_seconds(),
    })
}

fn unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub fn new_event_id(prefix: &str) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros();
    let seq = EVENT_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{now:x}-{seq:x}")
}
