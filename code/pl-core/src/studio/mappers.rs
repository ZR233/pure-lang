use anyhow::{Context, Result, bail};
use pl_protocol::{AgentStatus, Message, MessageContent, MessageRole};

use crate::studio::entities;
use crate::studio::ids::unix_seconds;
use crate::studio::records::{
    AgentSnapshotRecord, AgentTimelineEventRecord, ProjectRecord, SessionRecord,
    SessionRuntimeRecord, TimelineEventRecord,
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
    SessionRecord {
        id: model.id,
        project_id: model.project_id,
        title: model.title,
        mode: model.mode,
        updated_at: model.updated_at,
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
        updated_at: unix_seconds(),
    }
}

pub fn trace_event_kind_label(kind: &pl_protocol::TraceEventKind) -> &'static str {
    match kind {
        pl_protocol::TraceEventKind::TimelineItemStarted { .. } => "TimelineItemStarted",
        pl_protocol::TraceEventKind::TimelineItemDelta { .. } => "TimelineItemDelta",
        pl_protocol::TraceEventKind::TimelineItemCompleted { .. } => "TimelineItemCompleted",
        pl_protocol::TraceEventKind::TimelineItemFailed { .. } => "TimelineItemFailed",
    }
}

pub fn estimate_cost(
    prompt_tokens: u64,
    completion_tokens: u64,
    cached_prompt_tokens: u64,
    input_price_per_mtok: Option<f64>,
    output_price_per_mtok: Option<f64>,
    cache_read_price_per_mtok: Option<f64>,
) -> Option<f64> {
    let input_price = input_price_per_mtok?;
    let output_price = output_price_per_mtok?;
    let cache_price = cache_read_price_per_mtok?;
    let cached = cached_prompt_tokens.min(prompt_tokens);
    let uncached_input = prompt_tokens.saturating_sub(cached);
    Some(
        (uncached_input as f64 * input_price
            + completion_tokens as f64 * output_price
            + cached as f64 * cache_price)
            / 1_000_000.0,
    )
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
