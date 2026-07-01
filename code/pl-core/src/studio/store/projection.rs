use anyhow::{Result, bail};
use pl_protocol::{
    StudioEventEnvelope, StudioEventKind, StudioMessage, StudioPart, StudioPartStatus,
    StudioPartType,
};
use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter};

use crate::studio::entities;
use crate::studio::mappers::{
    studio_message_role_label, studio_message_status_label, studio_part_status_label,
    studio_part_type_label, studio_text_channel_label,
};
use crate::studio::records::AgentSnapshotRecord;

use super::{agent, runtime_usage, skill};
async fn upsert_studio_message_with_tx(
    tx: &sea_orm::DatabaseTransaction,
    message: &StudioMessage,
    sequence: i64,
) -> Result<()> {
    use entities::studio_message;
    let metadata_json = serde_json::to_string(&message.metadata)?;
    if let Some(existing) = studio_message::Entity::find_by_id(message.message_id.clone())
        .one(tx)
        .await?
    {
        validate_message_update(&existing, message, sequence, &metadata_json)?;
        let mut active: studio_message::ActiveModel = existing.into();
        active.status = Set(studio_message_status_label(message.status).to_string());
        active.updated_at = Set(message.updated_at);
        active.completed_at = Set(message.completed_at);
        active.error = Set(message.error.clone());
        active.metadata_json = Set(metadata_json);
        active.sequence = Set(sequence);
        active.update(tx).await?;
    } else {
        studio_message::ActiveModel {
            id: Set(message.message_id.clone()),
            session_id: Set(message.session_id.clone()),
            turn_id: Set(message.turn_id.clone()),
            role: Set(studio_message_role_label(message.role).to_string()),
            status: Set(studio_message_status_label(message.status).to_string()),
            created_at: Set(message.created_at),
            updated_at: Set(message.updated_at),
            completed_at: Set(message.completed_at),
            error: Set(message.error.clone()),
            metadata_json: Set(metadata_json),
            sequence: Set(sequence),
        }
        .insert(tx)
        .await?;
    }
    Ok(())
}

async fn delete_studio_message_with_tx(
    tx: &sea_orm::DatabaseTransaction,
    message_id: &str,
) -> Result<()> {
    use entities::{message_part, studio_message};
    message_part::Entity::delete_many()
        .filter(message_part::Column::MessageId.eq(message_id.to_string()))
        .exec(tx)
        .await?;
    studio_message::Entity::delete_by_id(message_id.to_string())
        .exec(tx)
        .await?;
    Ok(())
}

async fn upsert_message_part_with_tx(
    tx: &sea_orm::DatabaseTransaction,
    part: &StudioPart,
    sequence: i64,
) -> Result<()> {
    use entities::message_part;
    validate_part_activity_group(part)?;
    let attachments_json = serde_json::to_string(&part.attachments)?;
    let tool_json = optional_json_string(&part.tool)?;
    let agent_json = optional_json_string(&part.agent)?;
    let inference_json = optional_json_string(&part.inference)?;
    let plan_json = optional_json_string(&part.plan)?;
    let file_json = optional_json_string(&part.file)?;
    let usage_json = optional_json_string(&part.usage)?;
    if let Some(existing) = message_part::Entity::find_by_id(part.part_id.clone())
        .one(tx)
        .await?
    {
        validate_part_update(&existing, part, sequence)?;
        let mut active: message_part::ActiveModel = existing.into();
        active.revision = Set(part.revision as i64);
        active.status = Set(studio_part_status_label(part.status).to_string());
        active.updated_at = Set(part.updated_at);
        active.completed_at = Set(part.completed_at);
        active.error = Set(part.error.clone());
        active.activity_group_id = Set(part.activity_group_id.clone());
        active.text = Set(part.text.clone());
        active.attachments_json = Set(attachments_json);
        active.tool_json = Set(tool_json);
        active.agent_json = Set(agent_json);
        active.inference_json = Set(inference_json);
        active.plan_json = Set(plan_json);
        active.file_json = Set(file_json);
        active.usage_json = Set(usage_json);
        active.synthetic = Set(i32::from(part.synthetic));
        active.ignored = Set(i32::from(part.ignored));
        active.sequence = Set(sequence);
        active.update(tx).await?;
    } else {
        message_part::ActiveModel {
            id: Set(part.part_id.clone()),
            message_id: Set(part.message_id.clone()),
            session_id: Set(part.session_id.clone()),
            turn_id: Set(part.turn_id.clone()),
            part_type: Set(studio_part_type_label(part.part_type).to_string()),
            part_order: Set(part.order as i64),
            revision: Set(part.revision as i64),
            status: Set(studio_part_status_label(part.status).to_string()),
            created_at: Set(part.created_at),
            updated_at: Set(part.updated_at),
            completed_at: Set(part.completed_at),
            error: Set(part.error.clone()),
            text_channel: Set(part
                .text_channel
                .map(studio_text_channel_label)
                .map(str::to_string)),
            activity_group_id: Set(part.activity_group_id.clone()),
            text: Set(part.text.clone()),
            attachments_json: Set(attachments_json),
            tool_json: Set(tool_json),
            agent_json: Set(agent_json),
            inference_json: Set(inference_json),
            plan_json: Set(plan_json),
            file_json: Set(file_json),
            usage_json: Set(usage_json),
            synthetic: Set(i32::from(part.synthetic)),
            ignored: Set(i32::from(part.ignored)),
            sequence: Set(sequence),
        }
        .insert(tx)
        .await?;
    }
    Ok(())
}

async fn delete_message_part_with_tx(
    tx: &sea_orm::DatabaseTransaction,
    part_id: &str,
) -> Result<()> {
    use entities::message_part;
    message_part::Entity::delete_by_id(part_id.to_string())
        .exec(tx)
        .await?;
    Ok(())
}

fn optional_json_string<T: serde::Serialize>(value: &Option<T>) -> Result<Option<String>> {
    Ok(value.as_ref().map(serde_json::to_string).transpose()?)
}

fn validate_part_activity_group(part: &StudioPart) -> Result<()> {
    match part.part_type {
        StudioPartType::Tool => {
            if !matches!(part.activity_group_id.as_deref(), Some(group_id) if !group_id.is_empty())
            {
                bail!("tool part must include activityGroupId");
            }
        }
        StudioPartType::Text
        | StudioPartType::Reasoning
        | StudioPartType::Agent
        | StudioPartType::Turn
        | StudioPartType::Inference
        | StudioPartType::Plan
        | StudioPartType::File => {
            if part.activity_group_id.is_some() {
                bail!("non-tool part cannot include activityGroupId");
            }
        }
    }
    Ok(())
}

pub(super) async fn apply_studio_event_projection_with_tx(
    tx: &sea_orm::DatabaseTransaction,
    envelope: &StudioEventEnvelope,
) -> Result<()> {
    match &envelope.kind {
        StudioEventKind::MessageUpdated { message } => {
            upsert_studio_message_with_tx(tx, message, envelope.sequence as i64).await?;
        }
        StudioEventKind::MessageRemoved { message_id } => {
            delete_studio_message_with_tx(tx, message_id).await?;
        }
        StudioEventKind::MessagePartUpdated { part } => {
            upsert_message_part_with_tx(tx, part, envelope.sequence as i64).await?;
        }
        StudioEventKind::MessagePartRemoved { part_id, .. } => {
            delete_message_part_with_tx(tx, part_id).await?;
        }
        StudioEventKind::MessagePartDelta { .. } => {
            bail!("messagePartDelta is live-only and must not be projected");
        }
        StudioEventKind::SkillActivated { activation } => {
            if let Some(session_id) = envelope.session_id.as_deref() {
                skill::upsert_session_skill_with_tx(tx, session_id, activation).await?;
            }
        }
        StudioEventKind::AgentChanged { agent } => {
            if let Some(session_id) = envelope.session_id.as_deref() {
                agent::upsert_agent_snapshot_with_tx(
                    tx,
                    AgentSnapshotRecord {
                        id: agent.id.clone(),
                        session_id: session_id.to_string(),
                        path: agent.path.clone(),
                        parent_path: agent.parent_path.clone(),
                        role: agent.role.clone(),
                        task: agent.task.clone(),
                        status: agent.status,
                        summary: agent.summary.clone(),
                        depth: agent.depth as i32,
                        error: agent.error.clone(),
                        reason: agent.reason.clone(),
                        budget_limit_kind: agent.budget_limit_kind,
                        budget_usage: agent.budget_usage,
                        runtime_usage: agent
                            .runtime_usage
                            .clone()
                            .map(runtime_usage::runtime_usage_snapshot),
                        updated_at: agent.updated_at,
                    },
                )
                .await?;
            }
        }
        StudioEventKind::AgentTimelineChanged { event } => {
            if let Some(session_id) = envelope.session_id.as_deref()
                && let Some(record) =
                    agent::agent_timeline_event_record_from_event(session_id, event)
            {
                agent::insert_agent_event_with_tx(tx, record).await?;
            }
        }
        StudioEventKind::SessionRuntimeChanged { runtime } => {
            if let Some(session_id) = envelope.session_id.as_deref() {
                runtime_usage::upsert_session_runtime_snapshot_with_tx(tx, session_id, runtime)
                    .await?;
            }
        }
        StudioEventKind::InteractionChanged { .. }
        | StudioEventKind::PlanLifecycleChanged { .. }
        | StudioEventKind::TurnChanged { .. }
        | StudioEventKind::SessionHandoffChanged { .. }
        | StudioEventKind::SessionListChanged { .. }
        | StudioEventKind::McpHealthChanged { .. }
        | StudioEventKind::LspHealthChanged { .. }
        | StudioEventKind::Stale { .. } => {}
    }
    Ok(())
}

pub(super) fn studio_event_kind_label(kind: &StudioEventKind) -> &'static str {
    match kind {
        StudioEventKind::TurnChanged { .. } => "TurnChanged",
        StudioEventKind::MessageUpdated { .. } => "MessageUpdated",
        StudioEventKind::MessageRemoved { .. } => "MessageRemoved",
        StudioEventKind::MessagePartUpdated { .. } => "MessagePartUpdated",
        StudioEventKind::MessagePartRemoved { .. } => "MessagePartRemoved",
        StudioEventKind::MessagePartDelta { .. } => "MessagePartDelta",
        StudioEventKind::InteractionChanged { .. } => "InteractionChanged",
        StudioEventKind::AgentChanged { .. } => "AgentChanged",
        StudioEventKind::AgentTimelineChanged { .. } => "AgentTimelineChanged",
        StudioEventKind::SessionRuntimeChanged { .. } => "SessionRuntimeChanged",
        StudioEventKind::SkillActivated { .. } => "SkillActivated",
        StudioEventKind::PlanLifecycleChanged { .. } => "PlanLifecycleChanged",
        StudioEventKind::SessionHandoffChanged { .. } => "SessionHandoffChanged",
        StudioEventKind::SessionListChanged { .. } => "SessionListChanged",
        StudioEventKind::McpHealthChanged { .. } => "McpHealthChanged",
        StudioEventKind::LspHealthChanged { .. } => "LspHealthChanged",
        StudioEventKind::Stale { .. } => "Stale",
    }
}

fn validate_message_update(
    existing: &entities::studio_message::Model,
    incoming: &StudioMessage,
    incoming_sequence: i64,
    incoming_metadata_json: &str,
) -> Result<()> {
    if incoming_sequence <= existing.sequence {
        bail!("message sequence must increase");
    }
    if incoming.session_id != existing.session_id {
        bail!("message sessionId cannot change");
    }
    if incoming.turn_id != existing.turn_id {
        bail!("message turnId cannot change");
    }
    if studio_message_role_label(incoming.role) != existing.role {
        bail!("message role cannot change");
    }
    if incoming.created_at != existing.created_at {
        bail!("message createdAt cannot change");
    }
    if is_terminal_message_status_label(&existing.status)
        && (studio_message_status_label(incoming.status) != existing.status
            || incoming.updated_at != existing.updated_at
            || incoming.completed_at != existing.completed_at
            || incoming.error != existing.error
            || incoming_metadata_json != existing.metadata_json)
    {
        bail!("terminal message cannot change");
    }
    Ok(())
}

fn is_terminal_message_status_label(status: &str) -> bool {
    matches!(status, "completed" | "failed" | "cancelled")
}

fn validate_part_update(
    existing: &entities::message_part::Model,
    incoming: &StudioPart,
    incoming_sequence: i64,
) -> Result<()> {
    if incoming_sequence <= existing.sequence {
        bail!("part sequence must increase");
    }
    if (incoming.revision as i64) < existing.revision {
        bail!("part revision cannot decrease");
    }
    if incoming.message_id != existing.message_id {
        bail!("part messageId cannot change");
    }
    if incoming.session_id != existing.session_id {
        bail!("part sessionId cannot change");
    }
    if incoming.turn_id != existing.turn_id {
        bail!("part turnId cannot change");
    }
    if studio_part_type_label(incoming.part_type) != existing.part_type {
        bail!("part type cannot change");
    }
    if incoming.order as i64 != existing.part_order {
        bail!("part order cannot change");
    }
    if incoming.created_at != existing.created_at {
        bail!("part createdAt cannot change");
    }
    let incoming_text_channel = incoming
        .text_channel
        .map(studio_text_channel_label)
        .map(str::to_string);
    if incoming_text_channel != existing.text_channel {
        bail!("part textChannel cannot change");
    }
    if incoming.activity_group_id != existing.activity_group_id {
        bail!("part activityGroupId cannot change");
    }
    let existing_status = part_status_from_label_for_projection(&existing.status)?;
    if !valid_part_transition(existing_status, incoming.status) {
        bail!(
            "invalid part transition: {} -> {}",
            existing.status,
            studio_part_status_label(incoming.status)
        );
    }
    if is_terminal_part_status(existing_status) {
        validate_terminal_part_unchanged(existing, incoming)?;
    }
    Ok(())
}

fn is_terminal_part_status(status: StudioPartStatus) -> bool {
    matches!(
        status,
        StudioPartStatus::Completed
            | StudioPartStatus::Failed
            | StudioPartStatus::Interrupted
            | StudioPartStatus::Denied
            | StudioPartStatus::BudgetLimited
    )
}

fn validate_terminal_part_unchanged(
    existing: &entities::message_part::Model,
    incoming: &StudioPart,
) -> Result<()> {
    let incoming_completed_at = incoming.completed_at;
    if incoming.revision as i64 != existing.revision
        || studio_part_status_label(incoming.status) != existing.status
        || incoming_completed_at != existing.completed_at
        || incoming.error != existing.error
        || incoming.text != existing.text
        || serde_json::to_string(&incoming.attachments)? != existing.attachments_json
        || optional_json_string(&incoming.tool)? != existing.tool_json
        || optional_json_string(&incoming.agent)? != existing.agent_json
        || optional_json_string(&incoming.inference)? != existing.inference_json
        || optional_json_string(&incoming.plan)? != existing.plan_json
        || optional_json_string(&incoming.file)? != existing.file_json
        || optional_json_string(&incoming.usage)? != existing.usage_json
        || i32::from(incoming.synthetic) != existing.synthetic
        || i32::from(incoming.ignored) != existing.ignored
    {
        bail!("terminal part cannot change");
    }
    Ok(())
}

fn part_status_from_label_for_projection(label: &str) -> Result<StudioPartStatus> {
    match label {
        "started" => Ok(StudioPartStatus::Started),
        "streaming" => Ok(StudioPartStatus::Streaming),
        "awaitingApproval" => Ok(StudioPartStatus::AwaitingApproval),
        "approved" => Ok(StudioPartStatus::Approved),
        "denied" => Ok(StudioPartStatus::Denied),
        "running" => Ok(StudioPartStatus::Running),
        "completed" => Ok(StudioPartStatus::Completed),
        "failed" => Ok(StudioPartStatus::Failed),
        "interrupted" => Ok(StudioPartStatus::Interrupted),
        "budgetLimited" => Ok(StudioPartStatus::BudgetLimited),
        other => bail!("unsupported studio part status in db: {other}"),
    }
}

fn valid_part_transition(from: StudioPartStatus, to: StudioPartStatus) -> bool {
    use StudioPartStatus::{
        Approved, AwaitingApproval, BudgetLimited, Completed, Denied, Failed, Interrupted, Running,
        Started, Streaming,
    };

    match from {
        Completed | Failed | Interrupted | Denied | BudgetLimited => from == to,
        Started => matches!(
            to,
            Streaming
                | AwaitingApproval
                | Approved
                | Running
                | Completed
                | Failed
                | Interrupted
                | Denied
                | BudgetLimited
        ),
        Streaming | AwaitingApproval | Approved | Running => matches!(
            to,
            Streaming
                | AwaitingApproval
                | Approved
                | Running
                | Completed
                | Failed
                | Interrupted
                | Denied
                | BudgetLimited
        ),
    }
}
