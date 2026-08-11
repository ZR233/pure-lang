use std::collections::{BTreeMap, BTreeSet, VecDeque};

use pl_core::{
    ActiveKind, AgentActivityState, AgentIdentity, AgentRoleId, AgentSession, AgentSnapshot,
    AgentTurnOutcome, DurableMailboxEnvelope, MailboxDeliveryState, RestoredAgentRuntime,
    RestoredThreadSnapshot, ThreadActorState, ThreadCommit, ThreadCommitOutcome,
    ThreadContextState, ThreadId, ThreadRepository, TurnId, TurnOutcomeKind,
};
use pl_protocol::{
    Thread as ThreadRecord, ThreadItem, ThreadItemContent, ThreadNotification, ThreadSnapshot,
    Turn, TurnState,
};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter,
    QueryOrder, TransactionTrait,
};

use crate::PureError;
use crate::studio::StudioStore;
use crate::studio::entity::{interaction, item, thread, thread_input, turn};

mod billing;
mod context;
mod labels;

use billing::{
    aggregate_billing_usage, authoritative_turn_usage, persist_inference_billing, restore_billing,
    runtime_from_context,
};
use context::{
    SessionSnapshotAuditError, audit_session_snapshot, persist_session_snapshot,
    restore_session_snapshot, serialize_thread_metadata,
};
use labels::{
    activity_phase, interaction_kind_label, interaction_status_label, item_kind_label,
    item_status_label, lifecycle_from_status, outcome_columns, presentation_from_label,
    presentation_label, thread_status_from_label, thread_status_label, turn_phase_from_label,
    turn_state_columns,
};

/// Studio 单库对 canonical Thread 状态的 CAS repository。
#[derive(Clone)]
pub(in crate::studio) struct StudioAgentRepository {
    store: StudioStore,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::studio) struct StudioSessionRecoveryFailure {
    pub project_id: String,
    pub root_thread_id: String,
    pub agent_thread_id: String,
    pub detail: String,
}

impl StudioAgentRepository {
    pub(in crate::studio) fn new(store: StudioStore) -> Self {
        Self { store }
    }

    pub(in crate::studio) async fn audit_registered_sessions(
        &self,
    ) -> Result<Vec<StudioSessionRecoveryFailure>, PureError> {
        let models = thread::Entity::find()
            .filter(thread::Column::RuntimeRevision.is_not_null())
            .order_by_asc(thread::Column::CreatedAt)
            .order_by_asc(thread::Column::Id)
            .all(self.store.database())
            .await
            .map_err(store_error)?;
        self.session_recovery_failures(&models).await
    }

    async fn session_recovery_failures(
        &self,
        models: &[thread::Model],
    ) -> Result<Vec<StudioSessionRecoveryFailure>, PureError> {
        let mut failures = Vec::new();
        for model in models {
            match audit_session_snapshot(&self.store, &model.id).await {
                Ok(()) => {}
                Err(SessionSnapshotAuditError::Fatal(error)) => return Err(error),
                Err(SessionSnapshotAuditError::Corrupt(error)) => {
                    failures.push(StudioSessionRecoveryFailure {
                        project_id: model.project_id.clone(),
                        root_thread_id: model.root_thread_id.clone(),
                        agent_thread_id: model.id.clone(),
                        detail: error.to_string(),
                    });
                }
            }
        }
        Ok(failures)
    }

    async fn restore_inputs(
        &self,
        thread_id: &str,
    ) -> Result<
        (
            VecDeque<DurableMailboxEnvelope>,
            Option<DurableMailboxEnvelope>,
        ),
        PureError,
    > {
        let rows = thread_input::Entity::find()
            .filter(thread_input::Column::ThreadId.eq(thread_id))
            .filter(thread_input::Column::State.ne("consumed"))
            .order_by_asc(thread_input::Column::QueueOrdinal)
            .all(self.store.database())
            .await
            .map_err(store_error)?;
        let mut pending = VecDeque::new();
        let mut active = None;
        for row in rows {
            let input = input_from_model(row.clone())?;
            if row.state == "active" {
                if active.replace(input).is_some() {
                    return Err(store_error(format!(
                        "Thread {thread_id} has more than one active input"
                    )));
                }
            } else {
                pending.push_back(input);
            }
        }
        Ok((pending, active))
    }

    async fn restore_session(
        &self,
        model: &thread::Model,
    ) -> Result<ThreadContextState, PureError> {
        let session = restore_session_snapshot(&self.store, &model.id).await?;
        let billing_by_turn = restore_billing(&self.store, &model.id).await?;
        let usage = if billing_by_turn.is_empty() {
            serde_json::from_str(&model.usage_json)?
        } else {
            aggregate_billing_usage(billing_by_turn.values())
        };
        Ok(ThreadContextState {
            metadata: serde_json::from_str(&model.metadata_json)?,
            session: AgentSession::from_snapshot(session),
            usage,
            billing_by_turn,
            last_context_tokens: model.last_context_tokens.map(u64_from_i64).transpose()?,
            trace_sequence: u64_from_i64(model.trace_sequence)?,
            thread_revision: u64_from_i64(model.revision)?,
        })
    }

    async fn restore_thread_snapshot(
        &self,
        model: thread::Model,
        context: &ThreadContextState,
    ) -> Result<RestoredThreadSnapshot, PureError> {
        let thread_id = model.id.clone();
        let items = item::Entity::find()
            .filter(item::Column::ThreadId.eq(thread_id.clone()))
            .order_by_asc(item::Column::Ordinal)
            .all(self.store.database())
            .await
            .map_err(store_error)?
            .into_iter()
            .map(|row| serde_json::from_str(&row.payload_json).map_err(Into::into))
            .collect::<Result<Vec<ThreadItem>, PureError>>()?
            .into_iter()
            .filter(|item| !matches!(item.content, ThreadItemContent::ContextCompaction { .. }))
            .collect();
        let active_turn = turn::Entity::find()
            .filter(turn::Column::ThreadId.eq(thread_id.clone()))
            .filter(turn::Column::Status.is_in(["queued", "inProgress"]))
            .order_by_desc(turn::Column::Ordinal)
            .one(self.store.database())
            .await
            .map_err(store_error)?
            .map(turn_from_model)
            .transpose()?;
        let interactions = interaction::Entity::find()
            .filter(interaction::Column::ThreadId.eq(thread_id.clone()))
            .filter(interaction::Column::Status.eq("pending"))
            .order_by_asc(interaction::Column::CreatedAt)
            .all(self.store.database())
            .await
            .map_err(store_error)?
            .into_iter()
            .map(|row| {
                crate::studio::mappers::interaction_record(row)
                    .map_err(|error| store_error(error.to_string()))
            })
            .collect::<Result<Vec<_>, PureError>>()?;
        Ok(RestoredThreadSnapshot {
            snapshot: ThreadSnapshot {
                schema_version: pl_protocol::THREAD_SCHEMA_VERSION,
                revision: u64_from_i64(model.revision)?,
                thread: thread_from_model(model)?,
                active_turn,
                items,
                interactions,
                runtime: runtime_from_context(&thread_id, context),
            },
        })
    }
}

impl ThreadRepository for StudioAgentRepository {
    type Error = PureError;

    async fn restore_runtime(&self) -> Result<Vec<RestoredAgentRuntime>, Self::Error> {
        let models = thread::Entity::find()
            .filter(thread::Column::RuntimeRevision.is_not_null())
            .order_by_asc(thread::Column::CreatedAt)
            .order_by_asc(thread::Column::Id)
            .all(self.store.database())
            .await
            .map_err(store_error)?;
        let parents = models
            .iter()
            .map(|model| (model.id.clone(), model.parent_thread_id.clone()))
            .collect::<BTreeMap<_, _>>();
        let blocked_roots = self
            .session_recovery_failures(&models)
            .await?
            .into_iter()
            .map(|failure| failure.root_thread_id)
            .collect::<BTreeSet<_>>();
        let mut restored = Vec::with_capacity(models.len());
        for model in models {
            if blocked_roots.contains(&model.root_thread_id) {
                tracing::warn!(
                    root_thread_id = %model.root_thread_id,
                    agent_thread_id = %model.id,
                    "skipping agent tree with an invalid durable session snapshot"
                );
                continue;
            }
            let thread_id = ThreadId::new(model.id.clone())?;
            let (pending_inputs, active_input) = self.restore_inputs(thread_id.as_str()).await?;
            let active_turn = latest_turn(&self.store, thread_id.as_str(), true).await?;
            let last_turn = latest_turn(&self.store, thread_id.as_str(), false)
                .await?
                .map(turn_outcome_from_model)
                .transpose()?;
            let snapshot = AgentSnapshot {
                identity: AgentIdentity {
                    id: thread_id,
                    parent_id: model
                        .parent_thread_id
                        .as_ref()
                        .map(|id| ThreadId::new(id.clone()))
                        .transpose()?,
                    role: AgentRoleId::new(model.role.clone())?,
                    depth: thread_depth(&model.id, &parents)?,
                },
                lifecycle: lifecycle_from_status(&model.status)?,
                activity: restored_activity(&model.status, active_turn.as_ref(), &pending_inputs),
                active_turn_id: active_turn
                    .as_ref()
                    .map(|row| TurnId::new(row.id.clone()))
                    .transpose()?,
                pending_inputs: pending_inputs.len(),
                progress: None,
                last_turn,
                revision: u64_from_i64(model.runtime_revision.ok_or_else(|| {
                    store_error(format!("Thread {} actor is not registered", model.id))
                })?)?,
                event_sequence: u64_from_i64(model.event_sequence)?,
                updated_at: model.updated_at,
            };
            let session = self.restore_session(&model).await?;
            let thread_snapshot = self.restore_thread_snapshot(model, &session).await?;
            restored.push(RestoredAgentRuntime {
                state: ThreadActorState {
                    snapshot,
                    session,
                    pending_inputs,
                    active_input,
                },
                thread_snapshot: Some(thread_snapshot),
            });
        }
        Ok(restored)
    }

    async fn commit(&self, commit: ThreadCommit) -> Result<ThreadCommitOutcome, Self::Error> {
        persist_state_commit(&self.store, &commit).await
    }
}

pub(super) async fn persist_state_commit(
    store: &StudioStore,
    commit: &ThreadCommit,
) -> Result<ThreadCommitOutcome, PureError> {
    let thread_id = commit.agent_id.to_string();
    let tx = store.database().begin().await.map_err(store_error)?;
    let Some(existing) = thread::Entity::find_by_id(thread_id.clone())
        .one(&tx)
        .await
        .map_err(store_error)?
    else {
        tx.rollback().await.map_err(store_error)?;
        return Err(store_error(format!(
            "Thread {thread_id} must exist before runtime registration"
        )));
    };
    let actual_revision = existing.runtime_revision.map(u64_from_i64).transpose()?;
    if actual_revision != commit.expected_revision {
        tx.rollback().await.map_err(store_error)?;
        return Ok(ThreadCommitOutcome::RevisionConflict { actual_revision });
    }

    let mut active = existing.into_active_model();
    active.parent_thread_id = Set(commit
        .next_state
        .snapshot
        .identity
        .parent_id
        .as_ref()
        .map(ToString::to_string));
    active.role = Set(commit.next_state.snapshot.identity.role.to_string());
    active.status = Set(thread_status_label(&commit.next_state).to_string());
    active.revision = Set(i64_from_u64(commit.next_state.session.thread_revision)?);
    active.runtime_revision = Set(Some(i64_from_u64(commit.next_state.snapshot.revision)?));
    active.event_sequence = Set(i64_from_u64(commit.next_state.snapshot.event_sequence)?);
    active.metadata_json = Set(serialize_thread_metadata(
        &commit.next_state.session.metadata,
        &commit.next_state.session.session,
    )?);
    active.usage_json = Set(serde_json::to_string(&commit.next_state.session.usage)?);
    active.last_context_tokens = Set(commit
        .next_state
        .session
        .last_context_tokens
        .map(i64_from_u64)
        .transpose()?);
    active.trace_sequence = Set(i64_from_u64(commit.next_state.session.trace_sequence)?);
    active.updated_at = Set(commit.next_state.snapshot.updated_at);
    active.update(&tx).await.map_err(store_error)?;

    persist_inputs(&tx, &commit.next_state).await?;
    persist_state_turns(&tx, &commit.next_state).await?;
    persist_thread_notifications(&tx, commit).await?;
    persist_session_snapshot(&tx, commit).await?;
    persist_inference_billing(&tx, commit).await?;
    tx.commit().await.map_err(store_error)?;
    Ok(ThreadCommitOutcome::Applied)
}

async fn persist_inputs(
    tx: &sea_orm::DatabaseTransaction,
    state: &ThreadActorState,
) -> Result<(), PureError> {
    let thread_id = state.snapshot.identity.id.to_string();
    let existing = thread_input::Entity::find()
        .filter(thread_input::Column::ThreadId.eq(thread_id.clone()))
        .all(tx)
        .await
        .map_err(store_error)?;
    let mut live = BTreeSet::new();
    for input in &state.pending_inputs {
        live.insert(input.mail_id.clone());
        upsert_input(tx, &thread_id, input, false, state.snapshot.updated_at).await?;
    }
    if let Some(input) = &state.active_input {
        live.insert(input.mail_id.clone());
        upsert_input(tx, &thread_id, input, true, state.snapshot.updated_at).await?;
    }
    for row in existing {
        if live.contains(&row.mail_id) || row.state == "consumed" {
            continue;
        }
        let mut active = row.into_active_model();
        active.state = Set("consumed".to_string());
        active.consumed_at = Set(Some(state.snapshot.updated_at));
        active.update(tx).await.map_err(store_error)?;
    }
    Ok(())
}

async fn upsert_input(
    tx: &sea_orm::DatabaseTransaction,
    thread_id: &str,
    input: &DurableMailboxEnvelope,
    is_active: bool,
    updated_at: i64,
) -> Result<(), PureError> {
    let existing = thread_input::Entity::find_by_id(input.mail_id.clone())
        .one(tx)
        .await
        .map_err(store_error)?;
    if let Some(existing) = existing.as_ref()
        && existing.thread_id != thread_id
    {
        return Err(store_error(format!(
            "mail id {} belongs to another Thread",
            input.mail_id
        )));
    }
    let ordinal = match existing.as_ref() {
        Some(existing) => existing.queue_ordinal,
        None => next_input_ordinal(tx, thread_id).await?,
    };
    let (delivery_state, claimed_turn_id, checkpoint_seq) = if is_active {
        let (turn_id, sequence) =
            delivery_identity(&input.delivery_state).unwrap_or_else(|| (input.turn_id.clone(), 0));
        (
            "active",
            Some(turn_id.to_string()),
            Some(i64_from_u64(sequence)?),
        )
    } else {
        match &input.delivery_state {
            MailboxDeliveryState::Pending => ("queued", None, None),
            MailboxDeliveryState::Claimed {
                turn_id,
                checkpoint_seq,
            } => (
                "claimed",
                Some(turn_id.to_string()),
                Some(i64_from_u64(*checkpoint_seq)?),
            ),
            MailboxDeliveryState::Consumed {
                turn_id,
                checkpoint_seq,
            } => (
                "consumed",
                Some(turn_id.to_string()),
                Some(i64_from_u64(*checkpoint_seq)?),
            ),
        }
    };
    let active = thread_input::ActiveModel {
        id: Set(input.mail_id.clone()),
        thread_id: Set(thread_id.to_string()),
        mail_id: Set(input.mail_id.clone()),
        turn_id: Set(input.turn_id.to_string()),
        content: Set(input.message.clone()),
        metadata_json: Set(serialize_input_metadata(input)?),
        presentation: Set(presentation_label(input.presentation.clone()).to_string()),
        state: Set(delivery_state.to_string()),
        claimed_turn_id: Set(claimed_turn_id),
        checkpoint_seq: Set(checkpoint_seq),
        queue_ordinal: Set(ordinal),
        queued_at: Set(existing
            .as_ref()
            .map_or(input.queued_at, |row| row.queued_at)),
        claimed_at: Set((delivery_state != "queued").then_some(updated_at)),
        consumed_at: Set((delivery_state == "consumed").then_some(updated_at)),
    };
    match existing {
        Some(_) => active.update(tx).await.map_err(store_error)?,
        None => active.insert(tx).await.map_err(store_error)?,
    };
    Ok(())
}

async fn next_input_ordinal(
    tx: &sea_orm::DatabaseTransaction,
    thread_id: &str,
) -> Result<i64, PureError> {
    Ok(thread_input::Entity::find()
        .filter(thread_input::Column::ThreadId.eq(thread_id))
        .order_by_desc(thread_input::Column::QueueOrdinal)
        .one(tx)
        .await
        .map_err(store_error)?
        .map_or(0, |row| row.queue_ordinal.saturating_add(1)))
}

fn input_from_model(model: thread_input::Model) -> Result<DurableMailboxEnvelope, PureError> {
    let delivery_state = match model.state.as_str() {
        "queued" => MailboxDeliveryState::Pending,
        "claimed" | "active" => MailboxDeliveryState::Claimed {
            turn_id: TurnId::new(
                model
                    .claimed_turn_id
                    .clone()
                    .unwrap_or_else(|| model.turn_id.clone()),
            )?,
            checkpoint_seq: model
                .checkpoint_seq
                .map(u64_from_i64)
                .transpose()?
                .unwrap_or(0),
        },
        other => return Err(store_error(format!("cannot restore input state {other}"))),
    };
    let (metadata, queue_coalescing_key) = deserialize_input_metadata(&model.metadata_json)?;
    Ok(DurableMailboxEnvelope {
        mail_id: model.mail_id,
        turn_id: TurnId::new(model.turn_id)?,
        thread_id: ThreadId::new(model.thread_id)?,
        message: model.content,
        presentation: presentation_from_label(&model.presentation)?,
        metadata,
        queue_coalescing_key,
        delivery_state,
        queued_at: model.queued_at,
    })
}

const RUNTIME_INPUT_METADATA_KEY: &str = "$plAgentRuntime";
const INPUT_METADATA_PAYLOAD_KEY: &str = "payload";

fn serialize_input_metadata(input: &DurableMailboxEnvelope) -> Result<String, PureError> {
    let Some(key) = input.queue_coalescing_key.as_deref() else {
        return Ok(serde_json::to_string(&input.metadata)?);
    };
    let value = serde_json::json!({
        RUNTIME_INPUT_METADATA_KEY: {
            "queueCoalescingKey": key,
        },
        INPUT_METADATA_PAYLOAD_KEY: input.metadata,
    });
    Ok(serde_json::to_string(&value)?)
}

fn deserialize_input_metadata(
    input: &str,
) -> Result<(serde_json::Value, Option<String>), PureError> {
    let mut value: serde_json::Value = serde_json::from_str(input)?;
    let Some(object) = value.as_object_mut() else {
        return Ok((value, None));
    };
    let Some(key) = object
        .get(RUNTIME_INPUT_METADATA_KEY)
        .and_then(|runtime| runtime.get("queueCoalescingKey"))
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
    else {
        return Ok((value, None));
    };
    let payload = object
        .remove(INPUT_METADATA_PAYLOAD_KEY)
        .unwrap_or(serde_json::Value::Null);
    Ok((payload, Some(key)))
}

fn delivery_identity(state: &MailboxDeliveryState) -> Option<(TurnId, u64)> {
    match state {
        MailboxDeliveryState::Claimed {
            turn_id,
            checkpoint_seq,
        }
        | MailboxDeliveryState::Consumed {
            turn_id,
            checkpoint_seq,
        } => Some((turn_id.clone(), *checkpoint_seq)),
        MailboxDeliveryState::Pending => None,
    }
}

async fn persist_state_turns(
    tx: &sea_orm::DatabaseTransaction,
    state: &ThreadActorState,
) -> Result<(), PureError> {
    let thread_id = state.snapshot.identity.id.as_str();
    for input in &state.pending_inputs {
        persist_turn_projection(
            tx,
            TurnProjection {
                id: input.turn_id.as_str(),
                thread_id,
                status: "queued",
                phase: None,
                reason: None,
                usage: None,
                failure: None,
                budget_limit: None,
                rollover_compacted: None,
                rollover_compaction_error: None,
                metadata: Some(&input.metadata),
                started_at: None,
                completed_at: None,
                updated_at: input.queued_at,
                revision: state.session.thread_revision,
            },
        )
        .await?;
    }
    if let Some(turn_id) = state.snapshot.active_turn_id.as_ref() {
        persist_turn_projection(
            tx,
            TurnProjection {
                id: turn_id.as_str(),
                thread_id,
                status: "inProgress",
                phase: Some(activity_phase(state.snapshot.activity)),
                reason: None,
                usage: None,
                failure: None,
                budget_limit: None,
                rollover_compacted: None,
                rollover_compaction_error: None,
                metadata: None,
                started_at: Some(state.snapshot.updated_at),
                completed_at: None,
                updated_at: state.snapshot.updated_at,
                revision: state.session.thread_revision,
            },
        )
        .await?;
    }
    if let Some(outcome) = &state.snapshot.last_turn {
        let (status, reason) = outcome_columns(outcome);
        persist_turn_projection(
            tx,
            TurnProjection {
                id: outcome.turn_id.as_str(),
                thread_id,
                status,
                phase: None,
                reason,
                usage: Some(&outcome.usage),
                failure: outcome.failure.as_ref(),
                budget_limit: outcome.budget_limit.as_ref(),
                rollover_compacted: Some(outcome.rollover_compacted),
                rollover_compaction_error: outcome.rollover_compaction_error.as_deref(),
                metadata: None,
                started_at: None,
                completed_at: Some(outcome.finished_at),
                updated_at: outcome.finished_at,
                revision: state.session.thread_revision,
            },
        )
        .await?;
    }
    Ok(())
}

struct TurnProjection<'a> {
    id: &'a str,
    thread_id: &'a str,
    status: &'a str,
    phase: Option<&'a str>,
    reason: Option<&'a str>,
    usage: Option<&'a pl_model::TokenUsage>,
    failure: Option<&'a pl_protocol::TurnFailure>,
    budget_limit: Option<&'a pl_protocol::BudgetLimitSnapshot>,
    rollover_compacted: Option<bool>,
    rollover_compaction_error: Option<&'a str>,
    metadata: Option<&'a serde_json::Value>,
    started_at: Option<i64>,
    completed_at: Option<i64>,
    updated_at: i64,
    revision: u64,
}

async fn persist_turn_projection(
    tx: &sea_orm::DatabaseTransaction,
    projection: TurnProjection<'_>,
) -> Result<(), PureError> {
    let existing = turn::Entity::find_by_id(projection.id.to_string())
        .one(tx)
        .await
        .map_err(store_error)?;
    if let Some(existing) = existing.as_ref()
        && existing.thread_id != projection.thread_id
    {
        return Err(store_error(format!(
            "Turn {} belongs to another Thread",
            projection.id
        )));
    }
    let ordinal = match existing.as_ref() {
        Some(existing) => existing.ordinal,
        None => next_turn_ordinal(tx, projection.thread_id).await?,
    };
    let usage = authoritative_turn_usage(existing.as_ref(), projection.usage)?;
    let active = turn::ActiveModel {
        id: Set(projection.id.to_string()),
        thread_id: Set(projection.thread_id.to_string()),
        ordinal: Set(ordinal),
        revision: Set(i64_from_u64(projection.revision)?),
        status: Set(projection.status.to_string()),
        phase: Set(projection.phase.map(str::to_string)),
        reason: Set(projection.reason.map(str::to_string)),
        model_json: Set(existing.as_ref().and_then(|row| row.model_json.clone())),
        usage_json: Set(serde_json::to_string(&usage)?),
        failure_json: Set(projection.failure.map(serde_json::to_string).transpose()?),
        budget_limit_json: Set(match (projection.budget_limit, existing.as_ref()) {
            (Some(limit), _) => Some(serde_json::to_string(limit)?),
            (None, Some(row)) => row.budget_limit_json.clone(),
            (None, None) => None,
        }),
        rollover_compacted: Set(projection.rollover_compacted.map_or_else(
            || existing.as_ref().map_or(0, |row| row.rollover_compacted),
            |compacted| if compacted { 1 } else { 0 },
        )),
        rollover_compaction_error: Set(
            match (projection.rollover_compaction_error, existing.as_ref()) {
                (Some(error), _) => Some(error.to_string()),
                (None, Some(row)) => row.rollover_compaction_error.clone(),
                (None, None) => None,
            },
        ),
        metadata_json: Set(match (existing.as_ref(), projection.metadata) {
            (Some(row), None) => row.metadata_json.clone(),
            (_, metadata) => metadata.map(serde_json::to_string).transpose()?,
        }),
        started_at: Set(existing
            .as_ref()
            .and_then(|row| row.started_at)
            .or(projection.started_at)),
        updated_at: Set(projection.updated_at),
        completed_at: Set(projection.completed_at),
    };
    match existing {
        Some(_) => active.update(tx).await.map_err(store_error)?,
        None => active.insert(tx).await.map_err(store_error)?,
    };
    Ok(())
}

async fn persist_thread_notifications(
    tx: &sea_orm::DatabaseTransaction,
    commit: &ThreadCommit,
) -> Result<(), PureError> {
    for envelope in &commit.facts.notifications {
        match &envelope.notification {
            ThreadNotification::TurnStarted { turn: value }
            | ThreadNotification::TurnUpdated { turn: value }
            | ThreadNotification::TurnCompleted { turn: value } => {
                persist_turn(tx, value, envelope.revision).await?;
            }
            ThreadNotification::ItemStarted { item: value }
            | ThreadNotification::ItemCompleted { item: value } => {
                persist_item(tx, value).await?;
            }
            ThreadNotification::InteractionChanged { interaction: value } => {
                persist_interaction(tx, value).await?;
            }
            ThreadNotification::ItemDelta { .. }
            | ThreadNotification::ThreadRuntimeUpdated { .. }
            | ThreadNotification::Lagged { .. } => {}
        }
    }
    Ok(())
}

async fn persist_turn(
    tx: &sea_orm::DatabaseTransaction,
    value: &Turn,
    revision: u64,
) -> Result<(), PureError> {
    let (status, phase, reason) = turn_state_columns(&value.state);
    persist_turn_projection(
        tx,
        TurnProjection {
            id: &value.id,
            thread_id: &value.thread_id,
            status,
            phase,
            reason,
            usage: None,
            failure: None,
            budget_limit: None,
            rollover_compacted: None,
            rollover_compaction_error: None,
            metadata: None,
            started_at: value.started_at,
            completed_at: value.completed_at,
            updated_at: value.updated_at,
            revision,
        },
    )
    .await
}

async fn persist_item(
    tx: &sea_orm::DatabaseTransaction,
    value: &ThreadItem,
) -> Result<(), PureError> {
    let existing = item::Entity::find_by_id(value.id.clone())
        .one(tx)
        .await
        .map_err(store_error)?;
    if let Some(existing) = existing.as_ref()
        && (existing.thread_id != value.thread_id || existing.turn_id != value.turn_id)
    {
        return Err(store_error(format!(
            "Item {} immutable identity changed",
            value.id
        )));
    }
    let ordinal = match existing.as_ref() {
        Some(existing) => existing.ordinal,
        None if value.ordinal > 0 => i64_from_u64(value.ordinal)?,
        None => next_item_ordinal(tx, &value.thread_id).await?,
    };
    let mut persisted = value.clone();
    persisted.ordinal = u64_from_i64(ordinal)?;
    let active = item::ActiveModel {
        id: Set(value.id.clone()),
        thread_id: Set(value.thread_id.clone()),
        turn_id: Set(value.turn_id.clone()),
        ordinal: Set(ordinal),
        revision: Set(i64_from_u64(value.revision)?),
        item_kind: Set(item_kind_label(&value.content).to_string()),
        status: Set(item_status_label(value.status).to_string()),
        payload_json: Set(serde_json::to_string(&persisted)?),
        created_at: Set(existing
            .as_ref()
            .map_or(value.created_at, |row| row.created_at)),
        updated_at: Set(value.updated_at),
        completed_at: Set(value.completed_at),
    };
    match existing {
        Some(_) => active.update(tx).await.map_err(store_error)?,
        None => active.insert(tx).await.map_err(store_error)?,
    };
    Ok(())
}

async fn persist_interaction(
    tx: &sea_orm::DatabaseTransaction,
    value: &pl_protocol::InteractionRequest,
) -> Result<(), PureError> {
    let existing = interaction::Entity::find_by_id(value.interaction_id.clone())
        .one(tx)
        .await
        .map_err(store_error)?;
    let active = interaction::ActiveModel {
        id: Set(value.interaction_id.clone()),
        thread_id: Set(value.scope.thread_id.clone()),
        turn_id: Set(value.scope.turn_id.clone()),
        item_id: Set(value.scope.item_id.clone()),
        tool_id: Set(value.scope.tool_id.clone()),
        agent_path: Set(value.scope.agent_path.clone()),
        kind: Set(interaction_kind_label(value.kind.clone()).to_string()),
        status: Set(interaction_status_label(value.status.clone()).to_string()),
        payload_json: Set(serde_json::to_string(&value.payload)?),
        resolution_json: Set(value
            .resolution
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?),
        created_at: Set(existing
            .as_ref()
            .map_or(value.created_at, |row| row.created_at)),
        updated_at: Set(value.updated_at),
        resolved_at: Set(value.resolved_at),
    };
    match existing {
        Some(_) => active.update(tx).await.map_err(store_error)?,
        None => active.insert(tx).await.map_err(store_error)?,
    };
    Ok(())
}

async fn latest_turn(
    store: &StudioStore,
    thread_id: &str,
    active: bool,
) -> Result<Option<turn::Model>, PureError> {
    let query = turn::Entity::find().filter(turn::Column::ThreadId.eq(thread_id));
    let query = if active {
        query.filter(turn::Column::Status.is_in(["queued", "inProgress"]))
    } else {
        query.filter(turn::Column::Status.is_in(["completed", "failed", "interrupted"]))
    };
    query
        .order_by_desc(turn::Column::Ordinal)
        .one(store.database())
        .await
        .map_err(store_error)
}

async fn next_turn_ordinal(
    tx: &sea_orm::DatabaseTransaction,
    thread_id: &str,
) -> Result<i64, PureError> {
    Ok(turn::Entity::find()
        .filter(turn::Column::ThreadId.eq(thread_id))
        .order_by_desc(turn::Column::Ordinal)
        .one(tx)
        .await
        .map_err(store_error)?
        .map_or(0, |row| row.ordinal.saturating_add(1)))
}

async fn next_item_ordinal(
    tx: &sea_orm::DatabaseTransaction,
    thread_id: &str,
) -> Result<i64, PureError> {
    Ok(item::Entity::find()
        .filter(item::Column::ThreadId.eq(thread_id))
        .order_by_desc(item::Column::Ordinal)
        .one(tx)
        .await
        .map_err(store_error)?
        .map_or(0, |row| row.ordinal.saturating_add(1)))
}

fn thread_depth(id: &str, parents: &BTreeMap<String, Option<String>>) -> Result<u32, PureError> {
    let mut current = id;
    let mut depth = 0_u32;
    let mut remaining = parents.len();
    while let Some(parent) = parents.get(current).and_then(Option::as_deref) {
        if remaining == 0 {
            return Err(store_error("Thread parent graph contains a cycle"));
        }
        if !parents.contains_key(parent) {
            return Err(store_error(format!("Thread parent {parent} is missing")));
        }
        remaining -= 1;
        depth = depth.saturating_add(1);
        current = parent;
    }
    Ok(depth)
}

fn restored_activity(
    status: &str,
    active_turn: Option<&turn::Model>,
    pending: &VecDeque<DurableMailboxEnvelope>,
) -> AgentActivityState {
    if status == "closed" || status == "failed" {
        return AgentActivityState::Idle;
    }
    match active_turn {
        Some(turn) if turn.status == "queued" => AgentActivityState::Queued,
        // 老数据兼容：waitingInteraction phase 的 Turn 在 turn_from_model 里已降级为
        // completed，不会进入 active turn 查询；这里不再映射该 phase。
        Some(turn) => match turn.phase.as_deref() {
            Some("runningTool") => AgentActivityState::Active(ActiveKind::WaitingTool),
            _ => AgentActivityState::Active(ActiveKind::Running),
        },
        None if !pending.is_empty() => AgentActivityState::Queued,
        None => AgentActivityState::Idle,
    }
}

fn turn_outcome_from_model(model: turn::Model) -> Result<AgentTurnOutcome, PureError> {
    let budget_limit = model
        .budget_limit_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()?;
    let kind = match model.status.as_str() {
        "completed" => TurnOutcomeKind::Completed,
        "failed" => TurnOutcomeKind::Failed,
        "interrupted" if budget_limit.is_some() => TurnOutcomeKind::BudgetLimited,
        "interrupted" => TurnOutcomeKind::Cancelled,
        other => return Err(store_error(format!("Turn {other} is not terminal"))),
    };
    Ok(AgentTurnOutcome {
        turn_id: TurnId::new(model.id)?,
        thread_id: ThreadId::new(model.thread_id)?,
        kind,
        reason: model.reason,
        failure: model
            .failure_json
            .map(|value| serde_json::from_str(&value))
            .transpose()?,
        budget_limit,
        rollover_compacted: model.rollover_compacted != 0,
        rollover_compaction_error: model.rollover_compaction_error,
        usage: serde_json::from_str(&model.usage_json)?,
        finished_at: model.completed_at.unwrap_or(model.updated_at),
    })
}

fn thread_from_model(model: thread::Model) -> Result<ThreadRecord, PureError> {
    Ok(ThreadRecord {
        id: model.id,
        project_id: model.project_id,
        title: model.title,
        mode: labels::thread_mode_from_label(&model.mode)?,
        root_thread_id: model.root_thread_id,
        parent_thread_id: model.parent_thread_id,
        role: model.role,
        agent_path: model.agent_path,
        status: thread_status_from_label(&model.status)?,
        created_at: model.created_at,
        updated_at: model.updated_at,
        archived: model.archived != 0,
    })
}

fn turn_from_model(model: turn::Model) -> Result<Turn, PureError> {
    let failure = model
        .failure_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()?;
    // 老数据兼容：schema v1 可能把等待交互的 Turn 存成
    // status=inProgress + phase=waitingInteraction。新设计下这种 Turn 应是 completed
    // （pending Interaction 是 completion boundary），读回时降级。
    let state = if model.status.as_str() == "inProgress"
        && model.phase.as_deref() == Some("waitingInteraction")
    {
        TurnState::Completed
    } else {
        match model.status.as_str() {
            "queued" => TurnState::Queued,
            "inProgress" => TurnState::InProgress {
                phase: turn_phase_from_label(model.phase.as_deref().unwrap_or("preparing"))?,
            },
            "completed" => TurnState::Completed,
            "failed" => TurnState::Failed {
                reason: model.reason.clone().unwrap_or_default(),
            },
            "interrupted" => TurnState::Interrupted {
                reason: model.reason.clone().unwrap_or_default(),
            },
            other => return Err(store_error(format!("unknown Turn status {other}"))),
        }
    };
    Ok(Turn {
        id: model.id,
        thread_id: model.thread_id,
        state,
        failure,
        started_at: model.started_at,
        updated_at: model.updated_at,
        completed_at: model.completed_at,
    })
}

fn u64_from_i64(value: i64) -> Result<u64, PureError> {
    u64::try_from(value).map_err(|error| store_error(error.to_string()))
}

fn i64_from_u64(value: u64) -> Result<i64, PureError> {
    i64::try_from(value).map_err(|error| store_error(error.to_string()))
}

fn store_error(error: impl std::fmt::Display) -> PureError {
    PureError::MemoryError(error.to_string())
}

#[cfg(test)]
mod outcome_tests {
    use super::*;
    use pl_core::MailboxPresentation;

    #[test]
    fn budget_limited_turn_restores_typed_rollover_state() {
        let limit = pl_protocol::BudgetLimitSnapshot {
            kind: pl_protocol::BudgetLimitKind::WallClock,
            usage: pl_protocol::BudgetUsage {
                model_steps: 4,
                tool_calls: 8,
                wait_calls: 2,
                elapsed_ms: 1_800_000,
            },
        };
        let outcome = turn_outcome_from_model(turn::Model {
            id: "turn-budget".to_string(),
            thread_id: "thread-budget".to_string(),
            ordinal: 0,
            revision: 1,
            status: "interrupted".to_string(),
            phase: None,
            reason: Some("active wall-clock budget reached".to_string()),
            model_json: None,
            usage_json: serde_json::to_string(&pl_model::TokenUsage::default()).unwrap(),
            failure_json: None,
            budget_limit_json: Some(serde_json::to_string(&limit).unwrap()),
            rollover_compacted: 1,
            rollover_compaction_error: None,
            metadata_json: None,
            started_at: Some(1),
            updated_at: 2,
            completed_at: Some(2),
        })
        .unwrap();

        assert_eq!(outcome.kind, TurnOutcomeKind::BudgetLimited);
        assert_eq!(outcome.budget_limit, Some(limit));
        assert!(outcome.rollover_compacted);
        assert_eq!(
            outcome.reason.as_deref(),
            Some("active wall-clock budget reached")
        );
    }

    #[test]
    fn input_metadata_round_trips_queue_coalescing_key_without_changing_payload() {
        let input = DurableMailboxEnvelope {
            mail_id: "mail:wake".to_string(),
            turn_id: TurnId::new("turn-wake").unwrap(),
            thread_id: ThreadId::new("thread-wake").unwrap(),
            message: "wake".to_string(),
            presentation: MailboxPresentation::Hidden,
            metadata: serde_json::json!({"kind": "taskWake"}),
            queue_coalescing_key: Some("task-run:wakes".to_string()),
            delivery_state: MailboxDeliveryState::Pending,
            queued_at: 1,
        };

        let stored = serialize_input_metadata(&input).unwrap();
        let (metadata, key) = deserialize_input_metadata(&stored).unwrap();

        assert_eq!(metadata, input.metadata);
        assert_eq!(key, input.queue_coalescing_key);
    }

    #[test]
    fn legacy_input_metadata_remains_unwrapped() {
        let stored = r#"{"kind":"legacy"}"#;
        let (metadata, key) = deserialize_input_metadata(stored).unwrap();

        assert_eq!(metadata, serde_json::json!({"kind": "legacy"}));
        assert_eq!(key, None);
    }
}
