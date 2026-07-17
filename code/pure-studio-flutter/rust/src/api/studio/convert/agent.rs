use crate::api::studio::types::{
    BridgeAgentSnapshotDto, BridgeAgentTimelineEventDto, BridgeAgentTimelinePayloadDto,
    BridgeTodoItemDto, BridgeTodoListSnapshotDto,
};
use anyhow::{Context, Result};
use pl_studio_runtime::{
    StudioAgentSnapshot, StudioAgentTimelineEvent, StudioAgentTimelineEventKind,
};
pub(crate) fn agent_bridge_dto(
    agent: pl_studio_runtime::AgentSnapshotRecord,
) -> BridgeAgentSnapshotDto {
    BridgeAgentSnapshotDto {
        id: agent.id,
        session_id: agent.session_id,
        path: agent.path,
        parent_path: agent.parent_path,
        role: agent.role,
        task: agent.task,
        status: agent.status.as_str().to_string(),
        summary: agent.summary,
        depth: agent.depth as u32,
        error: agent.error,
        reason: agent.reason,
        updated_at: agent.updated_at,
    }
}

pub(crate) fn agent_event_bridge_dto(
    event: pl_studio_runtime::AgentTimelineEventRecord,
) -> Result<BridgeAgentTimelineEventDto> {
    let payload = serde_json::from_str::<StudioAgentTimelineEvent>(&event.payload_json)
        .with_context(|| {
            format!(
                "invalid agent timeline payload: {event_id}",
                event_id = event.event_id
            )
        })
        .map(|event| bridge_agent_timeline_payload(event.kind))?;
    Ok(BridgeAgentTimelineEventDto {
        event_id: event.event_id,
        session_id: event.session_id,
        sequence: event.sequence.max(0) as u64,
        created_at: event.created_at,
        payload,
    })
}

pub(crate) fn bridge_agent_snapshot(snapshot: StudioAgentSnapshot) -> BridgeAgentSnapshotDto {
    BridgeAgentSnapshotDto {
        id: snapshot.id,
        session_id: snapshot.session_id,
        path: snapshot.path,
        parent_path: snapshot.parent_path,
        role: snapshot.role,
        task: snapshot.task,
        status: snapshot.status.as_str().to_string(),
        summary: snapshot.summary,
        depth: snapshot.depth,
        error: snapshot.error,
        reason: snapshot.reason,
        updated_at: snapshot.updated_at,
    }
}

pub(crate) fn bridge_agent_timeline_event(
    event: StudioAgentTimelineEvent,
) -> BridgeAgentTimelineEventDto {
    BridgeAgentTimelineEventDto {
        event_id: event.event_id,
        session_id: event.session_id,
        sequence: event.sequence,
        created_at: event.created_at,
        payload: bridge_agent_timeline_payload(event.kind),
    }
}

pub(crate) fn bridge_agent_timeline_payload(
    payload: StudioAgentTimelineEventKind,
) -> BridgeAgentTimelinePayloadDto {
    match payload {
        StudioAgentTimelineEventKind::SubAgentActivity {
            call_id,
            agent_id,
            path,
            parent_path,
            kind,
            status,
            message,
            timed_out,
            error,
        } => BridgeAgentTimelinePayloadDto::SubAgentActivity {
            call_id,
            agent_id,
            path,
            parent_path,
            kind: kind.as_str().to_string(),
            status: status.map(|status| status.as_str().to_string()),
            message,
            timed_out: timed_out.unwrap_or(false),
            error,
        },
        StudioAgentTimelineEventKind::TodoListUpdated { snapshot } => {
            BridgeAgentTimelinePayloadDto::TodoListUpdated {
                snapshot: BridgeTodoListSnapshotDto {
                    call_id: snapshot.call_id,
                    agent_id: snapshot.agent_id,
                    path: snapshot.path,
                    parent_path: snapshot.parent_path,
                    explanation: snapshot.explanation,
                    items: snapshot
                        .items
                        .into_iter()
                        .map(|item| BridgeTodoItemDto {
                            step: item.step,
                            status: item.status.as_str().to_string(),
                        })
                        .collect(),
                },
            }
        }
    }
}
