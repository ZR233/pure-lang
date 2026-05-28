use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use pl_core::{StudioAgentEventRecord, StudioStore, SubagentEventRecord};
use pl_protocol::AgentEvent;
use tauri::{AppHandle, Emitter};
use tokio::sync::broadcast::error::RecvError;

use crate::dto::AgentEventPayload;
use crate::mappers::subagent_status_label;

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
                if let Some(record) = subagent_event_record(&session_id, &event) {
                    let _ = store.record_subagent_event(record).await;
                }
                if let Some(record) = agent_event_record(&session_id, &event) {
                    let _ = store.record_agent_event(record).await;
                }
                let _ = app.emit(
                    "studio-agent-event",
                    AgentEventPayload {
                        session_id: session_id.clone(),
                        event,
                    },
                );
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

pub fn agent_event_record(session_id: &str, event: &AgentEvent) -> Option<StudioAgentEventRecord> {
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
            updated_at,
        } => Some(StudioAgentEventRecord {
            event_id: new_event_id("agent-event"),
            session_id: session_id.to_string(),
            agent_id: id.clone(),
            path: path.clone(),
            parent_path: parent_path.clone(),
            role: role.clone(),
            task: task.clone(),
            status: *status,
            summary: summary.clone(),
            depth: *depth as i32,
            error: error.clone(),
            created_at: *updated_at,
        }),
        _ => None,
    }
}

pub fn subagent_event_record(session_id: &str, event: &AgentEvent) -> Option<SubagentEventRecord> {
    match event {
        AgentEvent::SubagentStateChanged {
            id,
            parent_id,
            role,
            task,
            status,
            summary,
            depth,
            error,
            updated_at,
        } => Some(SubagentEventRecord {
            event_id: new_event_id("subagent-event"),
            session_id: session_id.to_string(),
            subagent_id: id.clone(),
            parent_id: parent_id.clone(),
            role: role.clone(),
            task: task.clone(),
            status: subagent_status_label(*status).to_string(),
            summary: summary.clone(),
            depth: *depth as i32,
            error: error.clone(),
            created_at: *updated_at,
        }),
        _ => None,
    }
}

pub fn new_event_id(prefix: &str) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros();
    let seq = EVENT_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{now:x}-{seq:x}")
}
