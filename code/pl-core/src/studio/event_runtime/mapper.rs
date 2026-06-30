use pl_protocol::{
    StudioAgentPart, StudioAgentTimelineEvent, StudioAgentTimelineEventKind, StudioAttachment,
    StudioInferencePart, StudioMessageRole, StudioMessageStatus, StudioPart, StudioPartDelta,
    StudioPartDeltaField, StudioPartStatus, StudioPartType, StudioPlanPart, StudioSessionSummary,
    StudioTextChannel, StudioToolPart, StudioTurnStatus,
};
use pl_trace::{
    AgentEvent, TraceAgentPart, TraceDelta, TraceInferencePart, TracePart, TracePartDeltaEvent,
    TracePartKind, TracePartSource, TracePartStatus, TraceTextChannel, TraceToolPart,
};

use crate::studio::timeline_actor::is_terminal_studio_part_status;

pub(super) fn studio_session_summary(
    session: crate::studio::SessionRecord,
) -> StudioSessionSummary {
    StudioSessionSummary {
        id: session.id,
        project_id: session.project_id,
        title: session.title,
        mode: session.mode,
        updated_at: session.updated_at,
        visibility: session.visibility.as_str().to_string(),
        parent_session_id: session.parent_session_id,
    }
}

pub(super) fn trace_part_is_user_text(item: &TracePart) -> bool {
    item.kind == TracePartKind::Text && item.text_channel == Some(TraceTextChannel::User)
}

pub(super) fn trace_part_delta_is_user_text(event: &TracePartDeltaEvent) -> bool {
    matches!(
        (&event.kind, &event.delta),
        (
            TracePartKind::Text,
            TraceDelta::Text {
                text_channel: TraceTextChannel::User,
                ..
            }
        )
    )
}

pub(super) fn studio_part_delta(
    event: TracePartDeltaEvent,
    part_id: String,
    revision: u64,
) -> StudioPartDelta {
    let field = match &event.delta {
        TraceDelta::Text { .. } => StudioPartDeltaField::Text,
        TraceDelta::Thinking { .. } => StudioPartDeltaField::ReasoningSummary,
        TraceDelta::ToolArguments { .. } => StudioPartDeltaField::ToolArguments,
        TraceDelta::ToolResult { .. } => StudioPartDeltaField::ToolResult,
        TraceDelta::Plan { .. } => StudioPartDeltaField::PlanContent,
    };
    let chunk_index = match &event.delta {
        TraceDelta::Thinking { chunk_index, .. } => Some(*chunk_index),
        TraceDelta::Text { .. }
        | TraceDelta::ToolArguments { .. }
        | TraceDelta::ToolResult { .. }
        | TraceDelta::Plan { .. } => None,
    };
    StudioPartDelta {
        part_id,
        revision,
        field,
        delta: trace_delta_text(event.delta),
        chunk_index,
    }
}

pub(super) fn trace_delta_matches_part(
    session_id: &str,
    event: &TracePartDeltaEvent,
    existing: &crate::studio::records::StudioPartRecord,
) -> bool {
    let part = &existing.part;
    part.session_id == session_id
        && part.turn_id == event.turn_id
        && part.message_id == message_id_for_trace_delta(event)
        && trace_delta_field_matches_part(event, part)
}

fn trace_delta_field_matches_part(event: &TracePartDeltaEvent, part: &StudioPart) -> bool {
    match (&event.kind, &event.delta, part.part_type, part.text_channel) {
        (
            TracePartKind::Text,
            TraceDelta::Text { text_channel, .. },
            StudioPartType::Text,
            Some(part_channel),
        ) => studio_text_channel(*text_channel) == part_channel,
        (TracePartKind::Thinking, TraceDelta::Thinking { .. }, StudioPartType::Reasoning, _) => {
            true
        }
        (TracePartKind::Tool, TraceDelta::ToolArguments { .. }, StudioPartType::Tool, _)
        | (TracePartKind::Tool, TraceDelta::ToolResult { .. }, StudioPartType::Tool, _) => true,
        (TracePartKind::Plan, TraceDelta::Plan { .. }, StudioPartType::Plan, _) => true,
        (
            TracePartKind::Text
            | TracePartKind::Thinking
            | TracePartKind::Tool
            | TracePartKind::Agent
            | TracePartKind::Turn
            | TracePartKind::Inference
            | TracePartKind::Plan,
            TraceDelta::Text { .. }
            | TraceDelta::Thinking { .. }
            | TraceDelta::ToolArguments { .. }
            | TraceDelta::ToolResult { .. }
            | TraceDelta::Plan { .. },
            StudioPartType::Text
            | StudioPartType::Reasoning
            | StudioPartType::Tool
            | StudioPartType::Agent
            | StudioPartType::Turn
            | StudioPartType::Inference
            | StudioPartType::Plan
            | StudioPartType::File,
            _,
        ) => false,
    }
}

fn trace_delta_text(delta: TraceDelta) -> String {
    match delta {
        TraceDelta::Text { delta, .. }
        | TraceDelta::Thinking { delta, .. }
        | TraceDelta::ToolArguments { delta }
        | TraceDelta::ToolResult { delta }
        | TraceDelta::Plan { delta } => delta,
    }
}

pub(super) fn studio_agent_timeline_event(
    session_id: &str,
    event: AgentEvent,
) -> StudioAgentTimelineEvent {
    let kind = match event {
        AgentEvent::CollabAgentSpawnBegin {
            call_id,
            sender_path,
            task_name,
            prompt,
            role,
            model,
            reasoning_effort,
            ..
        } => StudioAgentTimelineEventKind::SpawnBegin {
            call_id,
            sender_path,
            task_name,
            prompt,
            role,
            model,
            reasoning_effort,
        },
        AgentEvent::CollabAgentSpawnEnd {
            call_id,
            sender_path,
            agent_id,
            path,
            role,
            status,
            prompt,
            error,
            ..
        } => StudioAgentTimelineEventKind::SpawnEnd {
            call_id,
            sender_path,
            agent_id,
            path,
            role,
            status,
            prompt,
            error,
        },
        AgentEvent::CollabAgentInteractionBegin {
            call_id,
            sender_path,
            receiver_path,
            prompt,
            ..
        } => StudioAgentTimelineEventKind::InteractionBegin {
            call_id,
            sender_path,
            receiver_path,
            prompt,
        },
        AgentEvent::CollabAgentInteractionEnd {
            call_id,
            sender_path,
            receiver_path,
            status,
            prompt,
            error,
            ..
        } => StudioAgentTimelineEventKind::InteractionEnd {
            call_id,
            sender_path,
            receiver_path,
            status,
            prompt,
            error,
        },
        AgentEvent::CollabWaitingBegin {
            call_id,
            sender_path,
            ..
        } => StudioAgentTimelineEventKind::WaitingBegin {
            call_id,
            sender_path,
        },
        AgentEvent::CollabWaitingEnd {
            call_id,
            sender_path,
            timed_out,
            ..
        } => StudioAgentTimelineEventKind::WaitingEnd {
            call_id,
            sender_path,
            timed_out,
        },
        AgentEvent::CollabCloseBegin {
            call_id,
            sender_path,
            receiver_path,
            ..
        } => StudioAgentTimelineEventKind::CloseBegin {
            call_id,
            sender_path,
            receiver_path,
        },
        AgentEvent::CollabCloseEnd {
            call_id,
            sender_path,
            receiver_path,
            status,
            error,
            ..
        } => StudioAgentTimelineEventKind::CloseEnd {
            call_id,
            sender_path,
            receiver_path,
            status,
            error,
        },
        AgentEvent::TracePartStarted { .. }
        | AgentEvent::TracePartDelta { .. }
        | AgentEvent::TracePartCompleted { .. }
        | AgentEvent::TracePartFailed { .. }
        | AgentEvent::InteractionChanged { .. }
        | AgentEvent::AgentStateChanged { .. }
        | AgentEvent::AgentRuntimeUpdated { .. }
        | AgentEvent::SkillActivated { .. }
        | AgentEvent::TurnInterrupted { .. }
        | AgentEvent::TurnBudgetLimited { .. }
        | AgentEvent::Done
        | AgentEvent::Error { .. } => {
            unreachable!("non agent trace events are filtered before mapping")
        }
    };
    StudioAgentTimelineEvent {
        event_id: String::new(),
        session_id: session_id.to_string(),
        sequence: 0,
        created_at: 0,
        kind,
    }
}

pub(super) fn studio_part_from_trace_part(session_id: &str, item: TracePart) -> StudioPart {
    let source = item.source;
    let part_type = part_type_for_trace_kind(item.kind);
    let status = part_status_for_trace_status(item.status);
    let completed_at = is_terminal_studio_part_status(status).then_some(item.updated_at);
    let text = part_text(&item);
    let message_id = message_id_for_trace_part(&item);
    let part_id = part_id_for_trace_part(&item);
    let error = error_for_part_status(status, &item.content);
    let tool = item.tool.map(studio_tool_part);
    let agent = item.agent.map(studio_agent_part);
    let inference = item.inference.map(studio_inference_part);
    let plan = matches!(part_type, StudioPartType::Plan).then(|| StudioPlanPart {
        content: item.content.clone(),
    });
    StudioPart {
        part_id,
        message_id,
        session_id: session_id.to_string(),
        turn_id: item.turn_id,
        part_type,
        order: item.started_sequence,
        revision: item.revision,
        status,
        created_at: item.created_at,
        updated_at: item.updated_at,
        completed_at,
        error,
        text_channel: item.text_channel.map(studio_text_channel),
        activity_group_id: None,
        text,
        attachments: item
            .attachments
            .into_iter()
            .map(|attachment| StudioAttachment {
                id: attachment.id,
                media_type: attachment.media_type,
                filename: attachment.filename,
                width: attachment.width,
                height: attachment.height,
                byte_size: attachment.byte_size,
                data_url: attachment.data_url,
            })
            .collect(),
        tool,
        agent,
        inference,
        plan,
        file: None,
        usage: item.usage,
        synthetic: matches!(source, TracePartSource::Runtime)
            || matches!(part_type, StudioPartType::Turn | StudioPartType::Inference),
        ignored: false,
    }
}

pub(super) fn message_id_for_trace_part(item: &TracePart) -> String {
    let suffix = match message_role_for_trace_part(item) {
        StudioMessageRole::User => "user",
        StudioMessageRole::Assistant => "assistant",
        StudioMessageRole::System => "system",
    };
    format!("{}:{suffix}", item.turn_id)
}

fn part_id_for_trace_part(item: &TracePart) -> String {
    match (item.kind, item.text_channel) {
        (TracePartKind::Text, Some(TraceTextChannel::User)) => {
            format!("{}:user-text", item.turn_id)
        }
        (TracePartKind::Text, Some(TraceTextChannel::Commentary))
        | (TracePartKind::Text, Some(TraceTextChannel::Final))
        | (TracePartKind::Text, None)
        | (TracePartKind::Thinking, _)
        | (TracePartKind::Tool, _)
        | (TracePartKind::Agent, _)
        | (TracePartKind::Turn, _)
        | (TracePartKind::Inference, _)
        | (TracePartKind::Plan, _) => item.item_id.clone(),
    }
}

fn message_id_for_trace_delta(event: &TracePartDeltaEvent) -> String {
    let suffix = match event.kind {
        TracePartKind::Text => match &event.delta {
            TraceDelta::Text {
                text_channel: TraceTextChannel::User,
                ..
            } => "user",
            TraceDelta::Text {
                text_channel: TraceTextChannel::Commentary | TraceTextChannel::Final,
                ..
            } => "assistant",
            TraceDelta::Thinking { .. }
            | TraceDelta::ToolArguments { .. }
            | TraceDelta::ToolResult { .. }
            | TraceDelta::Plan { .. } => "assistant",
        },
        TracePartKind::Thinking
        | TracePartKind::Tool
        | TracePartKind::Agent
        | TracePartKind::Turn
        | TracePartKind::Inference
        | TracePartKind::Plan => "assistant",
    };
    format!("{}:{suffix}", event.turn_id)
}

pub(super) fn message_role_for_trace_part(item: &TracePart) -> StudioMessageRole {
    match item.kind {
        TracePartKind::Text => match item.text_channel {
            Some(TraceTextChannel::User) => StudioMessageRole::User,
            Some(TraceTextChannel::Commentary | TraceTextChannel::Final) | None => {
                StudioMessageRole::Assistant
            }
        },
        TracePartKind::Thinking
        | TracePartKind::Tool
        | TracePartKind::Agent
        | TracePartKind::Turn
        | TracePartKind::Inference
        | TracePartKind::Plan => StudioMessageRole::Assistant,
    }
}

pub(super) fn assistant_message_status_for_turn(
    status: StudioTurnStatus,
) -> Option<StudioMessageStatus> {
    match status {
        StudioTurnStatus::Completed => Some(StudioMessageStatus::Completed),
        StudioTurnStatus::Failed => Some(StudioMessageStatus::Failed),
        StudioTurnStatus::Cancelled => Some(StudioMessageStatus::Cancelled),
        StudioTurnStatus::Queued
        | StudioTurnStatus::ContextLoading
        | StudioTurnStatus::WaitingForModel
        | StudioTurnStatus::Streaming
        | StudioTurnStatus::RunningTool
        | StudioTurnStatus::WaitingForInteraction
        | StudioTurnStatus::Persisting => None,
    }
}

fn part_type_for_trace_kind(kind: TracePartKind) -> StudioPartType {
    match kind {
        TracePartKind::Text => StudioPartType::Text,
        TracePartKind::Thinking => StudioPartType::Reasoning,
        TracePartKind::Tool => StudioPartType::Tool,
        TracePartKind::Agent => StudioPartType::Agent,
        TracePartKind::Turn => StudioPartType::Turn,
        TracePartKind::Inference => StudioPartType::Inference,
        TracePartKind::Plan => StudioPartType::Plan,
    }
}

fn part_status_for_trace_status(status: TracePartStatus) -> StudioPartStatus {
    match status {
        TracePartStatus::Started => StudioPartStatus::Started,
        TracePartStatus::Streaming => StudioPartStatus::Streaming,
        TracePartStatus::AwaitingApproval => StudioPartStatus::AwaitingApproval,
        TracePartStatus::Approved => StudioPartStatus::Approved,
        TracePartStatus::Denied => StudioPartStatus::Denied,
        TracePartStatus::Running => StudioPartStatus::Running,
        TracePartStatus::Completed => StudioPartStatus::Completed,
        TracePartStatus::Failed => StudioPartStatus::Failed,
        TracePartStatus::Interrupted => StudioPartStatus::Interrupted,
        TracePartStatus::BudgetLimited => StudioPartStatus::BudgetLimited,
    }
}

fn studio_text_channel(channel: TraceTextChannel) -> StudioTextChannel {
    match channel {
        TraceTextChannel::User => StudioTextChannel::User,
        TraceTextChannel::Commentary => StudioTextChannel::Commentary,
        TraceTextChannel::Final => StudioTextChannel::Final,
    }
}

fn part_text(item: &TracePart) -> String {
    match item.kind {
        TracePartKind::Text | TracePartKind::Plan | TracePartKind::Turn => item.content.clone(),
        TracePartKind::Thinking => item
            .thinking_chunks
            .iter()
            .map(|chunk| chunk.content.as_str())
            .collect::<Vec<_>>()
            .join(""),
        TracePartKind::Tool | TracePartKind::Agent | TracePartKind::Inference => {
            item.content.clone()
        }
    }
}

fn studio_tool_part(tool: TraceToolPart) -> StudioToolPart {
    StudioToolPart {
        tool_call_id: tool.tool_call_id,
        call_id: tool.call_id,
        provider_item_id: tool.provider_item_id,
        name: tool.name,
        arguments: tool.arguments,
        result: tool.result,
        exit_code: tool.exit_code,
        timed_out: tool.timed_out,
        working_directory: tool.working_directory,
        denial_reason: tool.denial_reason,
    }
}

fn studio_agent_part(agent: TraceAgentPart) -> StudioAgentPart {
    StudioAgentPart {
        id: agent.id,
        path: agent.path,
        parent_path: agent.parent_path,
        role: agent.role,
        task: agent.task,
        status: agent.status,
        summary: agent.summary,
        depth: agent.depth,
        error: agent.error,
        reason: agent.reason,
    }
}

fn studio_inference_part(inference: TraceInferencePart) -> StudioInferencePart {
    StudioInferencePart {
        inference_id: inference.inference_id,
        model: inference.model,
    }
}

fn error_for_part_status(status: StudioPartStatus, content: &str) -> Option<String> {
    match status {
        StudioPartStatus::Failed
        | StudioPartStatus::Interrupted
        | StudioPartStatus::BudgetLimited => {
            (!content.trim().is_empty()).then(|| content.to_string())
        }
        StudioPartStatus::Started
        | StudioPartStatus::Streaming
        | StudioPartStatus::AwaitingApproval
        | StudioPartStatus::Approved
        | StudioPartStatus::Denied
        | StudioPartStatus::Running
        | StudioPartStatus::Completed => None,
    }
}
