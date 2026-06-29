use crate::api::studio::types::{
    BridgeAgentSnapshotDto, BridgeAgentTimelineEventDto, BridgeAgentTimelinePayloadDto,
};
use anyhow::{Context, Result};
use pl_protocol::{StudioAgentSnapshot, StudioAgentTimelineEvent, StudioAgentTimelineEventKind};
pub(crate) fn agent_bridge_dto(
    agent: pl_core::StudioAgentSnapshotRecord,
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
    event: pl_core::StudioAgentTimelineEventRecord,
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
        StudioAgentTimelineEventKind::SpawnBegin {
            call_id,
            sender_path,
            task_name,
            prompt,
            role,
            model,
            reasoning_effort,
        } => BridgeAgentTimelinePayloadDto::SpawnBegin {
            call_id,
            sender_path,
            task_name,
            prompt,
            role,
            model,
            reasoning_effort,
        },
        StudioAgentTimelineEventKind::SpawnEnd {
            call_id,
            sender_path,
            agent_id,
            path,
            role,
            status,
            prompt,
            error,
        } => BridgeAgentTimelinePayloadDto::SpawnEnd {
            call_id,
            sender_path,
            agent_id,
            path,
            role,
            status: status.as_str().to_string(),
            prompt,
            error,
        },
        StudioAgentTimelineEventKind::InteractionBegin {
            call_id,
            sender_path,
            receiver_path,
            prompt,
        } => BridgeAgentTimelinePayloadDto::InteractionBegin {
            call_id,
            sender_path,
            receiver_path,
            prompt,
        },
        StudioAgentTimelineEventKind::InteractionEnd {
            call_id,
            sender_path,
            receiver_path,
            status,
            prompt,
            error,
        } => BridgeAgentTimelinePayloadDto::InteractionEnd {
            call_id,
            sender_path,
            receiver_path,
            status: status.as_str().to_string(),
            prompt,
            error,
        },
        StudioAgentTimelineEventKind::WaitingBegin {
            call_id,
            sender_path,
        } => BridgeAgentTimelinePayloadDto::WaitingBegin {
            call_id,
            sender_path,
        },
        StudioAgentTimelineEventKind::WaitingEnd {
            call_id,
            sender_path,
            timed_out,
        } => BridgeAgentTimelinePayloadDto::WaitingEnd {
            call_id,
            sender_path,
            timed_out,
        },
        StudioAgentTimelineEventKind::CloseBegin {
            call_id,
            sender_path,
            receiver_path,
        } => BridgeAgentTimelinePayloadDto::CloseBegin {
            call_id,
            sender_path,
            receiver_path,
        },
        StudioAgentTimelineEventKind::CloseEnd {
            call_id,
            sender_path,
            receiver_path,
            status,
            error,
        } => BridgeAgentTimelinePayloadDto::CloseEnd {
            call_id,
            sender_path,
            receiver_path,
            status: status.as_str().to_string(),
            error,
        },
    }
}
