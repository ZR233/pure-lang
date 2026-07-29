use anyhow::Result;
use pl_protocol::{
    AgentStatus, BudgetLimitKind, SessionAgentPart, SessionAgentSnapshot, SessionAttachment,
    SessionContextCompaction, SessionMessage, SessionMessageRole, SessionMessageStatus,
    SessionPart, SessionPartContent, SessionPartDelta, SessionPartDeltaField, SessionPartStatus,
    SessionTextChannel, SessionToolPart, SessionTurn, SessionTurnStatus, SessionViewSnapshot,
    TokenUsageSnapshot,
};

use crate::api::studio::types::*;

pub(super) fn session_snapshot(snapshot: SessionViewSnapshot) -> Result<BridgeSessionViewSnapshot> {
    Ok(BridgeSessionViewSnapshot {
        schema_version: snapshot.schema_version,
        session_id: snapshot.session_id,
        through_sequence: snapshot.through_sequence,
        owner: snapshot.owner.map(|owner| BridgeSessionOwnerSnapshot {
            agent_id: owner.agent_id,
            role: owner.role,
        }),
        turn: snapshot.turn.map(turn),
        messages: snapshot
            .messages
            .into_iter()
            .map(message)
            .collect::<Result<_>>()?,
        parts: snapshot
            .parts
            .into_iter()
            .map(part)
            .collect::<Result<_>>()?,
        interactions: snapshot
            .interactions
            .into_iter()
            .map(super::interaction::interaction)
            .collect::<Result<_>>()?,
        agents: snapshot.agents.into_iter().map(agent_snapshot).collect(),
        timeline_events: snapshot
            .timeline_events
            .into_iter()
            .map(super::runtime::timeline_event)
            .collect(),
        runtime: snapshot.runtime.map(super::runtime::runtime_snapshot),
        activated_skills: snapshot
            .activated_skills
            .into_iter()
            .map(super::runtime::skill_activation)
            .collect(),
        plan_events: snapshot
            .plan_events
            .into_iter()
            .map(super::runtime::plan_event)
            .collect(),
    })
}

pub(super) fn message(value: SessionMessage) -> Result<BridgeSessionMessage> {
    Ok(BridgeSessionMessage {
        message_id: value.message_id,
        session_id: value.session_id,
        turn_id: value.turn_id,
        role: match value.role {
            SessionMessageRole::User => BridgeSessionMessageRole::User,
            SessionMessageRole::Assistant => BridgeSessionMessageRole::Assistant,
            SessionMessageRole::System => BridgeSessionMessageRole::System,
        },
        status: match value.status {
            SessionMessageStatus::Queued => BridgeSessionMessageStatus::Queued,
            SessionMessageStatus::Streaming => BridgeSessionMessageStatus::Streaming,
            SessionMessageStatus::Completed => BridgeSessionMessageStatus::Completed,
            SessionMessageStatus::Failed => BridgeSessionMessageStatus::Failed,
            SessionMessageStatus::Cancelled => BridgeSessionMessageStatus::Cancelled,
        },
        created_at: value.created_at,
        updated_at: value.updated_at,
        completed_at: value.completed_at,
        error: value.error,
        metadata_json: serde_json::to_string(&value.metadata)?,
    })
}

pub(super) fn part(value: SessionPart) -> Result<BridgeSessionPart> {
    Ok(BridgeSessionPart {
        part_id: value.part_id,
        message_id: value.message_id,
        session_id: value.session_id,
        turn_id: value.turn_id,
        order: value.order,
        revision: value.revision,
        status: part_status(value.status),
        created_at: value.created_at,
        updated_at: value.updated_at,
        completed_at: value.completed_at,
        error: value.error,
        content: part_content(value.content)?,
        usage: value.usage.map(token_usage),
        synthetic: value.synthetic,
        ignored: value.ignored,
    })
}

fn part_content(value: SessionPartContent) -> Result<BridgeSessionPartContent> {
    Ok(match value {
        SessionPartContent::Text {
            channel,
            text,
            attachments,
        } => BridgeSessionPartContent::Text {
            channel: text_channel(channel),
            text,
            attachments: attachments.into_iter().map(attachment).collect(),
        },
        SessionPartContent::Reasoning { text } => BridgeSessionPartContent::Reasoning { text },
        SessionPartContent::Tool { tool } => BridgeSessionPartContent::Tool {
            tool: tool_part(tool)?,
        },
        SessionPartContent::Agent { agent } => BridgeSessionPartContent::Agent {
            agent: agent_part(agent),
        },
        SessionPartContent::Turn => BridgeSessionPartContent::Turn,
        SessionPartContent::Inference {
            inference_id,
            model,
        } => BridgeSessionPartContent::Inference {
            inference_id,
            model,
        },
        SessionPartContent::Plan { content } => BridgeSessionPartContent::Plan { content },
        SessionPartContent::File { path, media_type } => {
            BridgeSessionPartContent::File { path, media_type }
        }
    })
}

fn tool_part(value: SessionToolPart) -> Result<BridgeSessionToolPart> {
    let arguments_json = normalize_json_text(&value.arguments)?;
    Ok(BridgeSessionToolPart {
        tool_call_id: value.tool_call_id,
        call_id: value.call_id,
        provider_item_id: value.provider_item_id,
        name: value.name,
        arguments_json,
        result: value.result,
        output_artifacts_json: value
            .output_artifacts
            .iter()
            .map(serde_json::to_string)
            .collect::<serde_json::Result<_>>()?,
        exit_code: value.exit_code,
        timed_out: value.timed_out,
        working_directory: value.working_directory,
        denial_reason: value.denial_reason,
        activity_group_id: value.activity_group_id,
    })
}

fn normalize_json_text(value: &str) -> Result<String> {
    let json = if value.trim().is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_str(value)?
    };
    Ok(serde_json::to_string(&json)?)
}

fn agent_part(value: SessionAgentPart) -> BridgeSessionAgentPart {
    BridgeSessionAgentPart {
        id: value.id,
        path: value.path,
        parent_path: value.parent_path,
        role: value.role,
        task: value.task,
        status: agent_status(value.status),
        summary: value.summary,
        depth: value.depth,
        error: value.error,
        reason: value.reason,
    }
}

pub(super) fn part_delta(value: SessionPartDelta) -> BridgeSessionPartDelta {
    BridgeSessionPartDelta {
        part_id: value.part_id,
        revision: value.revision,
        field: match value.field {
            SessionPartDeltaField::Text => BridgeSessionPartDeltaField::Text,
            SessionPartDeltaField::ReasoningSummary => {
                BridgeSessionPartDeltaField::ReasoningSummary
            }
            SessionPartDeltaField::PlanContent => BridgeSessionPartDeltaField::PlanContent,
            SessionPartDeltaField::ToolArguments => BridgeSessionPartDeltaField::ToolArguments,
            SessionPartDeltaField::ToolResult => BridgeSessionPartDeltaField::ToolResult,
        },
        delta: value.delta,
        chunk_index: value.chunk_index,
    }
}

pub(super) fn turn(value: SessionTurn) -> BridgeSessionTurn {
    BridgeSessionTurn {
        turn_id: value.turn_id,
        session_id: value.session_id,
        status: match value.status {
            SessionTurnStatus::Queued => BridgeSessionTurnStatus::Queued,
            SessionTurnStatus::ContextLoading => BridgeSessionTurnStatus::ContextLoading,
            SessionTurnStatus::WaitingForModel => BridgeSessionTurnStatus::WaitingForModel,
            SessionTurnStatus::Streaming => BridgeSessionTurnStatus::Streaming,
            SessionTurnStatus::WaitingForInteraction => {
                BridgeSessionTurnStatus::WaitingForInteraction
            }
            SessionTurnStatus::RunningTool => BridgeSessionTurnStatus::RunningTool,
            SessionTurnStatus::Persisting => BridgeSessionTurnStatus::Persisting,
            SessionTurnStatus::Completed => BridgeSessionTurnStatus::Completed,
            SessionTurnStatus::Failed => BridgeSessionTurnStatus::Failed,
            SessionTurnStatus::Cancelled => BridgeSessionTurnStatus::Cancelled,
        },
        reason: value.reason,
        updated_at: value.updated_at,
    }
}

pub(super) fn agent_snapshot(value: SessionAgentSnapshot) -> BridgeSessionAgentSnapshot {
    BridgeSessionAgentSnapshot {
        id: value.id,
        session_id: value.session_id,
        path: value.path,
        parent_path: value.parent_path,
        role: value.role,
        task: value.task,
        status: agent_status(value.status),
        summary: value.summary,
        depth: value.depth,
        error: value.error,
        reason: value.reason,
        budget_limit_kind: value.budget_limit_kind.map(budget_limit),
        budget_usage: value.budget_usage.map(|usage| BridgeBudgetUsage {
            model_steps: usage.model_steps,
            tool_calls: usage.tool_calls,
            wait_calls: usage.wait_calls,
            elapsed_ms: usage.elapsed_ms,
        }),
        runtime_usage: value.runtime_usage.map(super::runtime::runtime_usage),
        updated_at: value.updated_at,
    }
}

pub(super) fn context_compaction(
    value: SessionContextCompaction,
) -> BridgeSessionContextCompaction {
    BridgeSessionContextCompaction {
        before_tokens: value.before_tokens,
        after_tokens: value.after_tokens,
        compacted_at: value.compacted_at,
    }
}

pub(super) fn agent_status(value: AgentStatus) -> BridgeAgentStatus {
    match value {
        AgentStatus::Queued => BridgeAgentStatus::Queued,
        AgentStatus::Running => BridgeAgentStatus::Running,
        AgentStatus::Waiting => BridgeAgentStatus::Waiting,
        AgentStatus::Completed => BridgeAgentStatus::Completed,
        AgentStatus::Errored => BridgeAgentStatus::Errored,
        AgentStatus::Interrupted => BridgeAgentStatus::Interrupted,
        AgentStatus::Shutdown => BridgeAgentStatus::Shutdown,
        AgentStatus::NotFound => BridgeAgentStatus::NotFound,
    }
}

fn budget_limit(value: BudgetLimitKind) -> BridgeBudgetLimitKind {
    match value {
        BudgetLimitKind::ModelStep => BridgeBudgetLimitKind::ModelStep,
        BudgetLimitKind::ToolCall => BridgeBudgetLimitKind::ToolCall,
        BudgetLimitKind::Wait => BridgeBudgetLimitKind::Wait,
        BudgetLimitKind::WallClock => BridgeBudgetLimitKind::WallClock,
        BudgetLimitKind::AgentCount => BridgeBudgetLimitKind::AgentCount,
        BudgetLimitKind::AgentDepth => BridgeBudgetLimitKind::AgentDepth,
        BudgetLimitKind::Finalization => BridgeBudgetLimitKind::Finalization,
    }
}

fn part_status(value: SessionPartStatus) -> BridgeSessionPartStatus {
    match value {
        SessionPartStatus::Started => BridgeSessionPartStatus::Started,
        SessionPartStatus::Streaming => BridgeSessionPartStatus::Streaming,
        SessionPartStatus::AwaitingApproval => BridgeSessionPartStatus::AwaitingApproval,
        SessionPartStatus::Approved => BridgeSessionPartStatus::Approved,
        SessionPartStatus::Denied => BridgeSessionPartStatus::Denied,
        SessionPartStatus::Running => BridgeSessionPartStatus::Running,
        SessionPartStatus::Completed => BridgeSessionPartStatus::Completed,
        SessionPartStatus::Failed => BridgeSessionPartStatus::Failed,
        SessionPartStatus::Interrupted => BridgeSessionPartStatus::Interrupted,
        SessionPartStatus::BudgetLimited => BridgeSessionPartStatus::BudgetLimited,
    }
}

fn text_channel(value: SessionTextChannel) -> BridgeSessionTextChannel {
    match value {
        SessionTextChannel::User => BridgeSessionTextChannel::User,
        SessionTextChannel::Commentary => BridgeSessionTextChannel::Commentary,
        SessionTextChannel::Final => BridgeSessionTextChannel::Final,
    }
}

fn attachment(value: SessionAttachment) -> BridgeSessionAttachment {
    BridgeSessionAttachment {
        id: value.id,
        media_type: value.media_type,
        filename: value.filename,
        width: value.width,
        height: value.height,
        byte_size: value.byte_size,
        data_url: value.data_url,
    }
}

fn token_usage(value: TokenUsageSnapshot) -> BridgeTokenUsageSnapshot {
    BridgeTokenUsageSnapshot {
        prompt_tokens: value.prompt_tokens,
        completion_tokens: value.completion_tokens,
        cached_prompt_tokens: value.cached_prompt_tokens,
        total_tokens: value.total_tokens,
    }
}
