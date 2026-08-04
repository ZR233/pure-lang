use anyhow::{Result, bail};
use pl_core::{ModelContextItem, SessionContextMutation, SessionHistoryCommit};
use pl_protocol::{SessionEventEnvelope, SessionEventKind, SessionTurn, SessionTurnState};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, DatabaseTransaction,
    EntityTrait, QueryFilter, QueryOrder, TransactionTrait,
};

use crate::studio::entity::{
    session_history_checkpoint, session_history_item, session_history_turn,
};

/// 按接收顺序把一批会话事实写入同一个历史库事务。
pub(crate) async fn persist_history_batch(
    history_db: &DatabaseConnection,
    commits: &[SessionHistoryCommit],
) -> Result<()> {
    if !commits.iter().any(commit_has_history) {
        return Ok(());
    }
    let transaction = history_db.begin().await?;
    for commit in commits {
        persist_history_commit(&transaction, commit).await?;
    }
    transaction.commit().await?;
    Ok(())
}

fn commit_has_history(commit: &SessionHistoryCommit) -> bool {
    !commit.facts.items.is_empty() || commit.facts.context.is_some()
}

async fn persist_history_commit(
    transaction: &DatabaseTransaction,
    commit: &SessionHistoryCommit,
) -> Result<()> {
    for event in &commit.facts.items {
        persist_history_item(transaction, event).await?;
        if let SessionEventKind::TurnChanged { turn } = &event.kind {
            persist_turn_transition(
                transaction,
                turn,
                commit
                    .facts
                    .projection_snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.runtime.as_ref())
                    .map(|runtime| &runtime.usage.model),
                event.emitted_at,
            )
            .await?;
        }
    }
    if commit.facts.context.is_some() {
        persist_context_checkpoint(transaction, commit).await?;
    }
    Ok(())
}

async fn persist_history_item(
    transaction: &DatabaseTransaction,
    event: &SessionEventEnvelope,
) -> Result<()> {
    let sequence = event
        .position
        .durable_sequence()
        .ok_or_else(|| anyhow::anyhow!("transient session event cannot enter durable history"))?;
    let sequence = i64::try_from(sequence)?;
    let payload_json = serde_json::to_string(event)?;
    if let Some(existing) =
        session_history_item::Entity::find_by_id((event.session_id.clone(), sequence))
            .one(transaction)
            .await?
    {
        if existing.item_id == event.event_id && existing.payload_json == payload_json {
            return Ok(());
        }
        bail!(
            "history sequence conflict for session {} at {}",
            event.session_id,
            sequence
        );
    }

    session_history_item::ActiveModel {
        session_id: Set(event.session_id.clone()),
        sequence: Set(sequence),
        item_id: Set(event.event_id.clone()),
        turn_id: Set(event.turn_id.clone().unwrap_or_default()),
        item_kind: Set(history_item_kind(&event.kind).to_string()),
        payload_json: Set(payload_json),
        created_at: Set(event.emitted_at),
    }
    .insert(transaction)
    .await?;
    Ok(())
}

async fn persist_turn_transition(
    transaction: &DatabaseTransaction,
    turn: &SessionTurn,
    model: Option<&String>,
    emitted_at: i64,
) -> Result<()> {
    let status = turn_status(&turn.state);
    let error_json = turn_error_json(&turn.state)?;
    let completed_at = turn_completed_at(&turn.state, emitted_at);
    if let Some(existing) = session_history_turn::Entity::find()
        .filter(session_history_turn::Column::SessionId.eq(turn.session_id.clone()))
        .filter(session_history_turn::Column::TurnId.eq(turn.turn_id.clone()))
        .one(transaction)
        .await?
    {
        let mut active: session_history_turn::ActiveModel = existing.into();
        active.status = Set(status.to_string());
        if let Some(model) = model {
            active.model_json = Set(Some(serde_json::to_string(model)?));
        }
        active.error_json = Set(error_json);
        active.completed_at = Set(completed_at);
        active.update(transaction).await?;
        return Ok(());
    }

    let next_sequence = session_history_turn::Entity::find()
        .filter(session_history_turn::Column::SessionId.eq(turn.session_id.clone()))
        .order_by_desc(session_history_turn::Column::TurnSequence)
        .one(transaction)
        .await?
        .map_or(1_i64, |latest| latest.turn_sequence.saturating_add(1));
    session_history_turn::ActiveModel {
        session_id: Set(turn.session_id.clone()),
        turn_sequence: Set(next_sequence),
        turn_id: Set(turn.turn_id.clone()),
        status: Set(status.to_string()),
        model_json: Set(model.map(serde_json::to_string).transpose()?),
        error_json: Set(error_json),
        started_at: Set(emitted_at),
        completed_at: Set(completed_at),
    }
    .insert(transaction)
    .await?;
    Ok(())
}

async fn persist_context_checkpoint(
    transaction: &DatabaseTransaction,
    commit: &SessionHistoryCommit,
) -> Result<()> {
    let session_id = commit.facts.session_id.to_string();
    let revision = i64::try_from(commit.facts.revision)?;
    let through_sequence = i64::try_from(commit.facts.through_sequence)?;
    let context = match commit
        .facts
        .context
        .as_ref()
        .expect("caller checks context mutation")
    {
        SessionContextMutation::Replace { items } => items.clone(),
        SessionContextMutation::Append { items } => {
            let mut context: Vec<ModelContextItem> = session_history_checkpoint::Entity::find()
                .filter(session_history_checkpoint::Column::SessionId.eq(session_id.clone()))
                .order_by_desc(session_history_checkpoint::Column::Revision)
                .one(transaction)
                .await?
                .map(|checkpoint| serde_json::from_str(&checkpoint.context_json))
                .transpose()?
                .unwrap_or_default();
            context.extend(items.iter().cloned());
            context
        }
    };
    let context_json = serde_json::to_string(&context)?;
    if let Some(existing) =
        session_history_checkpoint::Entity::find_by_id((session_id.clone(), revision))
            .one(transaction)
            .await?
    {
        if existing.through_sequence == through_sequence && existing.context_json == context_json {
            return Ok(());
        }
        bail!(
            "history checkpoint revision conflict for session {} at {}",
            session_id,
            revision
        );
    }
    session_history_checkpoint::ActiveModel {
        session_id: Set(session_id),
        revision: Set(revision),
        through_sequence: Set(through_sequence),
        context_json: Set(context_json),
        created_at: Set(commit.next_state.snapshot.updated_at),
    }
    .insert(transaction)
    .await?;
    Ok(())
}

fn history_item_kind(kind: &SessionEventKind) -> &'static str {
    match kind {
        SessionEventKind::TurnChanged { .. } => "turnChanged",
        SessionEventKind::MessageChanged { .. } => "messageChanged",
        SessionEventKind::MessageRemoved { .. } => "messageRemoved",
        SessionEventKind::PartChanged { .. } => "partChanged",
        SessionEventKind::PartRemoved { .. } => "partRemoved",
        SessionEventKind::PartDelta { .. } => "partDelta",
        SessionEventKind::InteractionChanged { .. } => "interactionChanged",
        SessionEventKind::AgentChanged { .. } => "agentChanged",
        SessionEventKind::TimelineEventAppended { .. } => "timelineEventAppended",
        SessionEventKind::RuntimeChanged { .. } => "runtimeChanged",
        SessionEventKind::SkillActivated { .. } => "skillActivated",
        SessionEventKind::PlanChanged { .. } => "planChanged",
        SessionEventKind::ContextCompacted { .. } => "contextCompacted",
        SessionEventKind::ErrorOccurred { .. } => "errorOccurred",
    }
}

fn turn_status(state: &SessionTurnState) -> &'static str {
    match state {
        SessionTurnState::Queued => "queued",
        SessionTurnState::InProgress { .. } => "inProgress",
        SessionTurnState::Completed => "completed",
        SessionTurnState::Failed { .. } => "failed",
        SessionTurnState::Cancelled { .. } => "cancelled",
    }
}

fn turn_error_json(state: &SessionTurnState) -> Result<Option<String>> {
    match state {
        SessionTurnState::Failed { reason } | SessionTurnState::Cancelled { reason } => {
            Ok(Some(serde_json::to_string(&serde_json::json!({
                "reason": reason
            }))?))
        }
        SessionTurnState::Queued
        | SessionTurnState::InProgress { .. }
        | SessionTurnState::Completed => Ok(None),
    }
}

fn turn_completed_at(state: &SessionTurnState, emitted_at: i64) -> Option<i64> {
    match state {
        SessionTurnState::Completed
        | SessionTurnState::Failed { .. }
        | SessionTurnState::Cancelled { .. } => Some(emitted_at),
        SessionTurnState::Queued | SessionTurnState::InProgress { .. } => None,
    }
}
