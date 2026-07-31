use pl_protocol::{
    SessionAgentPart, SessionAttachment, SessionPart, SessionPartContent, SessionPartStatus,
    SessionTextChannel, SessionToolPart,
};
use pl_trace::{TracePart, TracePartKind, TracePartSource, TracePartStatus, TraceTextChannel};

pub(super) fn session_part(
    session_id: &str,
    message_id: &str,
    item: &TracePart,
    failure: Option<&str>,
) -> SessionPart {
    SessionPart {
        part_id: item.item_id.clone(),
        message_id: message_id.to_string(),
        session_id: session_id.to_string(),
        turn_id: item.turn_id.clone(),
        order: item.started_sequence,
        revision: item.revision,
        status: failure.map_or_else(
            || session_part_status(item.status),
            |_| SessionPartStatus::Failed,
        ),
        created_at: item.created_at,
        updated_at: item.updated_at,
        completed_at: is_terminal(item.status).then_some(item.updated_at),
        error: failure.map(str::to_string),
        content: session_part_content(item),
        usage: item.usage.clone(),
        synthetic: item.source == TracePartSource::Runtime,
        ignored: matches!(item.kind, TracePartKind::Turn | TracePartKind::Inference),
    }
}

fn session_part_content(item: &TracePart) -> SessionPartContent {
    match item.kind {
        TracePartKind::Text => SessionPartContent::Text {
            channel: session_text_channel(item.text_channel.unwrap_or(TraceTextChannel::Final)),
            text: item.content.clone(),
            attachments: item
                .attachments
                .iter()
                .map(|attachment| SessionAttachment {
                    id: attachment.id.clone(),
                    media_type: attachment.media_type.clone(),
                    filename: attachment.filename.clone(),
                    width: attachment.width,
                    height: attachment.height,
                    byte_size: attachment.byte_size,
                    data_url: attachment.data_url.clone(),
                })
                .collect(),
        },
        TracePartKind::Thinking => SessionPartContent::Reasoning {
            summary: item
                .thinking_chunks
                .iter()
                .map(|chunk| chunk.content.clone())
                .collect(),
            content: item
                .reasoning_content_chunks
                .iter()
                .map(|chunk| chunk.content.clone())
                .collect(),
        },
        TracePartKind::Tool => SessionPartContent::Tool {
            tool: item
                .tool
                .as_ref()
                .map(|tool| SessionToolPart {
                    tool_call_id: tool.tool_call_id.clone(),
                    call_id: tool.call_id.clone(),
                    provider_item_id: tool.provider_item_id.clone(),
                    name: tool.name.clone(),
                    arguments: tool.arguments.clone(),
                    result: tool.result.clone(),
                    output_artifacts: tool.output_artifacts.clone(),
                    exit_code: tool.exit_code,
                    timed_out: tool.timed_out,
                    working_directory: tool.working_directory.clone(),
                    denial_reason: tool.denial_reason.clone(),
                })
                .unwrap_or_else(|| SessionToolPart {
                    tool_call_id: item.item_id.clone(),
                    call_id: None,
                    provider_item_id: None,
                    name: "tool".to_string(),
                    arguments: String::new(),
                    result: None,
                    output_artifacts: Vec::new(),
                    exit_code: None,
                    timed_out: false,
                    working_directory: None,
                    denial_reason: None,
                }),
        },
        TracePartKind::Agent => SessionPartContent::Agent {
            agent: item
                .agent
                .as_ref()
                .map(|agent| SessionAgentPart {
                    id: agent.id.clone(),
                    path: agent.path.clone(),
                    parent_path: agent.parent_path.clone(),
                    role: agent.role.clone(),
                    task: agent.task.clone(),
                    status: agent.status,
                    summary: agent.summary.clone(),
                    depth: agent.depth,
                    error: agent.error.clone(),
                    reason: agent.reason.clone(),
                })
                .unwrap_or_else(|| SessionAgentPart {
                    id: item.item_id.clone(),
                    path: String::new(),
                    parent_path: None,
                    role: String::new(),
                    task: String::new(),
                    status: pl_protocol::AgentStatus::Running,
                    summary: None,
                    depth: 0,
                    error: None,
                    reason: None,
                }),
        },
        TracePartKind::Turn => SessionPartContent::Turn,
        TracePartKind::Inference => item
            .inference
            .as_ref()
            .map(|inference| SessionPartContent::Inference {
                inference_id: inference.inference_id.clone(),
                model: inference.model.clone(),
            })
            .unwrap_or_else(|| SessionPartContent::Inference {
                inference_id: item.item_id.clone(),
                model: String::new(),
            }),
        TracePartKind::Plan => SessionPartContent::Plan {
            content: item.content.clone(),
        },
    }
}

fn session_text_channel(channel: TraceTextChannel) -> SessionTextChannel {
    match channel {
        TraceTextChannel::User => SessionTextChannel::User,
        TraceTextChannel::Commentary => SessionTextChannel::Commentary,
        TraceTextChannel::Final => SessionTextChannel::Final,
    }
}

fn session_part_status(status: TracePartStatus) -> SessionPartStatus {
    match status {
        TracePartStatus::Started => SessionPartStatus::Started,
        TracePartStatus::Streaming => SessionPartStatus::Streaming,
        TracePartStatus::AwaitingApproval => SessionPartStatus::AwaitingApproval,
        TracePartStatus::Approved => SessionPartStatus::Approved,
        TracePartStatus::Denied => SessionPartStatus::Denied,
        TracePartStatus::Running => SessionPartStatus::Running,
        TracePartStatus::Completed => SessionPartStatus::Completed,
        TracePartStatus::Failed => SessionPartStatus::Failed,
        TracePartStatus::Interrupted => SessionPartStatus::Interrupted,
        TracePartStatus::BudgetLimited => SessionPartStatus::BudgetLimited,
    }
}

fn is_terminal(status: TracePartStatus) -> bool {
    matches!(
        status,
        TracePartStatus::Completed
            | TracePartStatus::Failed
            | TracePartStatus::Interrupted
            | TracePartStatus::BudgetLimited
            | TracePartStatus::Denied
    )
}
