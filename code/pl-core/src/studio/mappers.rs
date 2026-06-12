use anyhow::{Context, Result, bail};
use pl_protocol::{
    AgentStatus, InteractionPayload, InteractionRequest, InteractionResolution, InteractionScope,
    InteractionStatus, Message, MessageContent, MessageRole, RuntimeCostAmount,
    RuntimeUsageSnapshot, StudioEventEnvelope, StudioTurnStatus,
};

use crate::studio::entities;
use crate::studio::ids::unix_seconds;
use crate::studio::records::{
    AgentSnapshotRecord, AgentTimelineEventRecord, ProjectRecord, SessionHandoffKind,
    SessionHandoffRecord, SessionHandoffStatus, SessionRecord, SessionRuntimeRecord,
    SessionSkillRecord, SessionVisibility, StudioEventRecord, StudioTurnRecord,
    TimelineEventRecord,
};

pub fn project_record(model: entities::project::Model) -> ProjectRecord {
    ProjectRecord {
        id: model.id,
        name: model.name,
        path: model.path,
        updated_at: model.updated_at,
    }
}

pub fn session_record(model: entities::session::Model) -> SessionRecord {
    let instruction_snapshot = model
        .instruction_snapshot_json
        .as_deref()
        .and_then(|json| serde_json::from_str(json).ok());
    SessionRecord {
        id: model.id,
        project_id: model.project_id,
        title: model.title,
        mode: model.mode,
        updated_at: model.updated_at,
        visibility: session_visibility_from_label(&model.visibility),
        instruction_snapshot,
    }
}

fn session_visibility_from_label(label: &str) -> SessionVisibility {
    match label {
        "active" => SessionVisibility::Active,
        "handoffOrigin" => SessionVisibility::HandoffOrigin,
        "archived" => SessionVisibility::Archived,
        _ => SessionVisibility::Archived,
    }
}

pub fn session_handoff_record(model: entities::session_handoff::Model) -> SessionHandoffRecord {
    SessionHandoffRecord {
        id: model.id,
        project_id: model.project_id,
        origin_session_id: model.origin_session_id,
        target_session_id: model.target_session_id,
        kind: session_handoff_kind_from_label(&model.kind),
        plan_id: model.plan_id,
        status: session_handoff_status_from_label(&model.status),
        created_at: model.created_at,
        updated_at: model.updated_at,
    }
}

fn session_handoff_kind_from_label(label: &str) -> SessionHandoffKind {
    match label {
        "planImplementation" => SessionHandoffKind::PlanImplementation,
        _ => SessionHandoffKind::PlanImplementation,
    }
}

fn session_handoff_status_from_label(label: &str) -> SessionHandoffStatus {
    match label {
        "pending" => SessionHandoffStatus::Pending,
        "running" => SessionHandoffStatus::Running,
        "completed" => SessionHandoffStatus::Completed,
        "failed" => SessionHandoffStatus::Failed,
        "cancelled" => SessionHandoffStatus::Cancelled,
        _ => SessionHandoffStatus::Failed,
    }
}

pub fn agent_snapshot_record(model: entities::agent::Model) -> AgentSnapshotRecord {
    let budget_usage = model
        .budget_usage_json
        .as_deref()
        .and_then(|json| serde_json::from_str(json).ok());
    AgentSnapshotRecord {
        id: model.id,
        session_id: model.session_id,
        path: model.path,
        parent_path: model.parent_path,
        role: model.role,
        task: model.task,
        status: agent_status_from_label(&model.status),
        summary: model.summary,
        depth: model.depth,
        error: model.error,
        reason: model.reason,
        budget_limit_kind: model
            .budget_limit_kind
            .as_deref()
            .and_then(budget_limit_kind_from_label),
        budget_usage,
        runtime_usage: None,
        updated_at: model.updated_at,
    }
}

pub fn agent_runtime_snapshot_record(
    model: entities::agent_runtime_snapshot::Model,
) -> RuntimeUsageSnapshot {
    RuntimeUsageSnapshot {
        model: model.model,
        context_window: model.context_window.map(|value| value as u64),
        latest_context_tokens: model.latest_context_tokens as u64,
        prompt_tokens: model.prompt_tokens as u64,
        completion_tokens: model.completion_tokens as u64,
        cached_prompt_tokens: model.cached_prompt_tokens as u64,
        total_tokens: model.total_tokens as u64,
        estimated_costs: costs_from_json(&model.estimated_costs_json),
        has_unpriced_usage: model.has_unpriced_usage != 0,
        updated_at: model.updated_at,
    }
}

pub fn agent_timeline_event_record(
    model: entities::agent_event::Model,
) -> AgentTimelineEventRecord {
    AgentTimelineEventRecord {
        event_id: model.id,
        session_id: model.session_id,
        sequence: model.sequence,
        kind: model.kind,
        agent_id: model.agent_id,
        path: model.path,
        parent_path: model.parent_path,
        payload_json: model.payload_json,
        created_at: model.created_at,
    }
}

fn budget_limit_kind_from_label(label: &str) -> Option<pl_protocol::BudgetLimitKind> {
    match label {
        "modelStep" => Some(pl_protocol::BudgetLimitKind::ModelStep),
        "toolCall" => Some(pl_protocol::BudgetLimitKind::ToolCall),
        "wait" => Some(pl_protocol::BudgetLimitKind::Wait),
        "wallClock" => Some(pl_protocol::BudgetLimitKind::WallClock),
        "agentCount" => Some(pl_protocol::BudgetLimitKind::AgentCount),
        "agentDepth" => Some(pl_protocol::BudgetLimitKind::AgentDepth),
        "finalization" => Some(pl_protocol::BudgetLimitKind::Finalization),
        _ => None,
    }
}

pub fn agent_status_from_label(status: &str) -> AgentStatus {
    match status {
        "queued" => AgentStatus::Queued,
        "running" => AgentStatus::Running,
        "waiting" => AgentStatus::Waiting,
        "completed" => AgentStatus::Completed,
        "errored" => AgentStatus::Errored,
        "interrupted" => AgentStatus::Interrupted,
        "shutdown" => AgentStatus::Shutdown,
        "notFound" => AgentStatus::NotFound,
        _ => AgentStatus::Errored,
    }
}

pub fn timeline_event_record(model: entities::timeline_event::Model) -> TimelineEventRecord {
    TimelineEventRecord {
        id: model.id,
        session_id: model.session_id,
        sequence: model.sequence,
        created_at: model.created_at,
        kind: model.kind,
        payload_json: model.payload_json,
    }
}

pub fn studio_event_record(model: entities::studio_event::Model) -> StudioEventRecord {
    StudioEventRecord {
        id: model.id,
        project_id: model.project_id,
        session_id: model.session_id,
        turn_id: model.turn_id,
        sequence: model.sequence,
        created_at: model.created_at,
        kind: model.kind,
        payload_json: model.payload_json,
    }
}

pub fn studio_event_envelope(record: StudioEventRecord) -> Result<StudioEventEnvelope> {
    let envelope = serde_json::from_str::<StudioEventEnvelope>(&record.payload_json)
        .with_context(|| format!("failed to parse studio event {}", record.id))?;
    Ok(envelope)
}

pub fn studio_turn_record(model: entities::turn::Model) -> StudioTurnRecord {
    StudioTurnRecord {
        id: model.id,
        session_id: model.session_id,
        status: studio_turn_status_from_label(&model.status),
        reason: model.reason,
        created_at: model.created_at,
        updated_at: model.updated_at,
        completed_at: model.completed_at,
    }
}

pub fn studio_turn_status_from_label(label: &str) -> StudioTurnStatus {
    match label {
        "queued" => StudioTurnStatus::Queued,
        "contextLoading" => StudioTurnStatus::ContextLoading,
        "waitingForModel" => StudioTurnStatus::WaitingForModel,
        "streaming" => StudioTurnStatus::Streaming,
        "waitingForInteraction" => StudioTurnStatus::WaitingForInteraction,
        "runningTool" => StudioTurnStatus::RunningTool,
        "persisting" => StudioTurnStatus::Persisting,
        "completed" => StudioTurnStatus::Completed,
        "failed" => StudioTurnStatus::Failed,
        "cancelled" => StudioTurnStatus::Cancelled,
        _ => StudioTurnStatus::Failed,
    }
}

pub fn interaction_record(model: entities::interaction::Model) -> Result<InteractionRequest> {
    let payload = serde_json::from_str::<InteractionPayload>(&model.payload_json)
        .with_context(|| format!("failed to parse interaction payload: {}", model.id))?;
    let resolution = model
        .resolution_json
        .as_deref()
        .map(serde_json::from_str::<InteractionResolution>)
        .transpose()
        .with_context(|| format!("failed to parse interaction resolution: {}", model.id))?;
    Ok(InteractionRequest {
        interaction_id: model.id,
        kind: interaction_kind_from_label(&model.kind)?,
        status: interaction_status_from_label(&model.status)?,
        scope: InteractionScope {
            session_id: model.session_id,
            turn_id: model.turn_id,
            item_id: model.item_id,
            tool_id: model.tool_id,
            agent_path: model.agent_path,
        },
        payload,
        created_at: model.created_at,
        updated_at: model.updated_at,
        resolved_at: model.resolved_at,
        resolution,
    })
}

fn interaction_kind_from_label(label: &str) -> Result<pl_protocol::InteractionKind> {
    match label {
        "userInput" => Ok(pl_protocol::InteractionKind::UserInput),
        "toolApproval" => Ok(pl_protocol::InteractionKind::ToolApproval),
        "planConfirmation" => Ok(pl_protocol::InteractionKind::PlanConfirmation),
        other => bail!("unsupported interaction kind in studio db: {other}"),
    }
}

fn interaction_status_from_label(label: &str) -> Result<InteractionStatus> {
    match label {
        "pending" => Ok(InteractionStatus::Pending),
        "resolved" => Ok(InteractionStatus::Resolved),
        "cancelled" => Ok(InteractionStatus::Cancelled),
        "expired" => Ok(InteractionStatus::Expired),
        other => bail!("unsupported interaction status in studio db: {other}"),
    }
}

pub fn session_runtime_record(
    model: entities::session_runtime_snapshot::Model,
) -> SessionRuntimeRecord {
    SessionRuntimeRecord {
        session_id: model.session_id,
        model: model.model,
        context_window: model.context_window.map(|value| value as u64),
        latest_context_tokens: model.latest_context_tokens as u64,
        prompt_tokens: model.prompt_tokens as u64,
        completion_tokens: model.completion_tokens as u64,
        cached_prompt_tokens: model.cached_prompt_tokens as u64,
        total_tokens: model.total_tokens as u64,
        currency: model.currency,
        estimated_cost: model.estimated_cost,
        estimated_costs: costs_from_json(&model.estimated_costs_json),
        has_unpriced_usage: model.has_unpriced_usage != 0,
        updated_at: model.updated_at,
    }
}

pub fn session_skill_record(model: entities::session_skill::Model) -> SessionSkillRecord {
    SessionSkillRecord {
        session_id: model.session_id,
        skill_name: model.skill_name,
        source: model.source,
        path: model.path,
        first_turn_id: model.first_turn_id,
        last_turn_id: model.last_turn_id,
        last_tool_call_id: model.last_tool_call_id,
        activated_at: model.activated_at,
        updated_at: model.updated_at,
    }
}

pub fn default_session_runtime_record(
    session_id: &str,
    model: Option<&pl_model::ModelInfo>,
) -> SessionRuntimeRecord {
    SessionRuntimeRecord {
        session_id: session_id.to_string(),
        model: model
            .map(|model| model.slug.clone())
            .unwrap_or_else(|| "unknown".to_string()),
        context_window: model.and_then(pl_model::ModelInfo::resolved_context_window),
        latest_context_tokens: 0,
        prompt_tokens: 0,
        completion_tokens: 0,
        cached_prompt_tokens: 0,
        total_tokens: 0,
        currency: model.and_then(|model| model.currency.clone()),
        estimated_cost: None,
        estimated_costs: Vec::new(),
        has_unpriced_usage: false,
        updated_at: unix_seconds(),
    }
}

pub fn trace_event_kind_label(kind: &pl_protocol::TraceEventKind) -> &'static str {
    match kind {
        pl_protocol::TraceEventKind::TimelineItemStarted { .. } => "TimelineItemStarted",
        pl_protocol::TraceEventKind::TimelineItemDelta { .. } => "TimelineItemDelta",
        pl_protocol::TraceEventKind::TimelineItemCompleted { .. } => "TimelineItemCompleted",
        pl_protocol::TraceEventKind::TimelineItemFailed { .. } => "TimelineItemFailed",
        pl_protocol::TraceEventKind::PlanLifecycleChanged { .. } => "PlanLifecycleChanged",
        pl_protocol::TraceEventKind::InteractionChanged { .. } => "InteractionChanged",
        pl_protocol::TraceEventKind::SkillActivated { .. } => "SkillActivated",
        pl_protocol::TraceEventKind::EnabledToolsRecorded { .. } => "EnabledToolsRecorded",
    }
}

pub fn costs_from_json(json: &str) -> Vec<RuntimeCostAmount> {
    let mut costs = serde_json::from_str::<Vec<RuntimeCostAmount>>(json).unwrap_or_default();
    costs.sort_by(|left, right| left.currency.cmp(&right.currency));
    costs
}

pub fn costs_to_json(costs: &[RuntimeCostAmount]) -> String {
    serde_json::to_string(costs).unwrap_or_else(|_| "[]".to_string())
}

pub fn row_to_message(row: entities::message::Model) -> Result<Message> {
    let role = match row.role.as_str() {
        "system" => MessageRole::System,
        "user" => MessageRole::User,
        "assistant" => MessageRole::Assistant,
        "tool" => MessageRole::Tool,
        other => bail!("unsupported message role in studio db: {other}"),
    };
    let metadata = serde_json::from_str(&row.metadata_json)
        .with_context(|| format!("failed to parse message metadata: {}", row.id))?;
    Ok(Message {
        role,
        content: MessageContent::Text(row.content),
        reasoning_content: row.reasoning_content,
        metadata,
    })
}

pub fn message_to_row_parts(message: &Message) -> Result<(String, String)> {
    let role = match message.role {
        MessageRole::System => "system",
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::Tool => "tool",
    };
    let content = match &message.content {
        MessageContent::Text(text) => text.clone(),
        MessageContent::MultiPart(parts) => serde_json::to_string(parts)?,
    };
    Ok((role.to_string(), content))
}
