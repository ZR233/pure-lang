use std::collections::BTreeSet;

use pl_core::{
    AgentSubmissionPage, AgentSubmissionRecord, DurableMailboxEnvelope, MailboxCommand,
    MailboxDeliveryState, RestoredAgentRuntime, ThreadActorState, ThreadCommit, ThreadId,
    ThreadRepository, TurnId,
};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, IntoActiveModel, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect,
};

use crate::PureError;
use crate::studio::StudioStore;
use crate::studio::entity::{thread, thread_input, thread_submission};
use crate::studio::store::object::{ThreadCommitReceipt, load_object, put_object};

mod billing;
mod context;
mod conversion;
pub(super) mod labels;
mod write_behind;

use billing::persist_inference_billing;
use context::{SessionSnapshotAuditError, persist_session_snapshot, serialize_thread_metadata};
use labels::presentation_label;

use input_metadata::serialize_input_metadata;
use projection::{persist_state_turns, persist_thread_notifications};
pub(in crate::studio) use write_behind::ThreadWriteBehindWriter;

/// Studio 单库对 canonical Thread 状态的 write-behind repository。
///
/// commit 只进入 [`ThreadWriteBehindWriter`] 队列，由后台批量事务落库；
/// 内存 actor state 是唯一权威实例。
#[derive(Clone)]
pub(in crate::studio) struct StudioAgentRepository {
    store: StudioStore,
    writer: ThreadWriteBehindWriter,
    model_performance: Option<crate::studio::runtime::ModelPerformanceOwner>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::studio) struct StudioSessionRecoveryFailure {
    pub project_id: String,
    pub root_thread_id: String,
    pub agent_thread_id: String,
    pub detail: String,
}

mod input_metadata;
mod projection;
mod restore;
#[cfg(test)]
mod unit_tests;

impl StudioAgentRepository {
    /// write-behind writer 共享实例构造：与 Agent runtime、ProductEventBus 使用
    /// 同一进程级 writer，恢复基线 seed 进该实例。
    #[cfg(test)]
    pub(in crate::studio) fn with_writer(
        store: StudioStore,
        writer: ThreadWriteBehindWriter,
    ) -> Self {
        Self {
            writer,
            store,
            model_performance: None,
        }
    }

    pub(in crate::studio) fn with_writer_and_performance(
        store: StudioStore,
        writer: ThreadWriteBehindWriter,
        model_performance: crate::studio::runtime::ModelPerformanceOwner,
    ) -> Self {
        Self {
            writer,
            store,
            model_performance: Some(model_performance),
        }
    }

    /// write-behind writer 句柄；关机排空与进度查询使用。
    pub(in crate::studio) fn writer(&self) -> &ThreadWriteBehindWriter {
        &self.writer
    }
}

impl ThreadRepository for StudioAgentRepository {
    type Error = PureError;

    /// 只恢复启动钉住集合：存在 pending input、pending Interaction、活动 Turn
    /// 或被活动协作/续轮引用的 Thread。其余 Thread 惰性驻留。
    async fn restore_runtime(&self) -> Result<Vec<RestoredAgentRuntime>, Self::Error> {
        let pinned = self.pinned_thread_ids().await?;
        if pinned.is_empty() {
            return Ok(Vec::new());
        }
        let models = thread::Entity::find()
            .filter(thread::Column::RuntimeRevision.is_not_null())
            .filter(thread::Column::Id.is_in(pinned))
            .order_by_asc(thread::Column::CreatedAt)
            .order_by_asc(thread::Column::Id)
            .all(self.store.database())
            .await
            .map_err(store_error)?;
        let parents = self.ancestor_parents(&models).await?;
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
            restored.push(self.restore_model(model, &parents).await?);
        }
        Ok(restored)
    }

    /// 按需恢复单个已注册 Thread；不存在、未注册 runtime 或 session 损坏时返回 `None`。
    async fn restore_thread(
        &self,
        thread_id: &ThreadId,
    ) -> Result<Option<RestoredAgentRuntime>, Self::Error> {
        let Some(model) = thread::Entity::find_by_id(thread_id.to_string())
            .one(self.store.database())
            .await
            .map_err(store_error)?
        else {
            return Ok(None);
        };
        if model.runtime_revision.is_none() {
            return Ok(None);
        }
        match self.audit_thread_recovery_payloads(&model.id).await {
            Ok(()) => {}
            Err(SessionSnapshotAuditError::Corrupt(_)) => {
                tracing::warn!(
                    agent_thread_id = %model.id,
                    "refusing to lazily restore a thread with an invalid durable payload"
                );
                return Ok(None);
            }
            Err(SessionSnapshotAuditError::Fatal(error)) => return Err(error),
        }
        let parents = self.ancestor_parents(std::slice::from_ref(&model)).await?;
        Ok(Some(self.restore_model(model, &parents).await?))
    }

    async fn commit(&self, commit: ThreadCommit) -> Result<(), Self::Error> {
        self.writer
            .accept_thread_with_backpressure(commit.clone())
            .await?;
        if let (Some(owner), Some(inference)) =
            (&self.model_performance, commit.facts.inference.as_ref())
        {
            let projection = commit.facts.projection_snapshot.as_ref().ok_or_else(|| {
                store_error("inference commit is missing its canonical Thread projection")
            })?;
            if let Err(error) = owner
                .record_inference(
                    &projection.thread.root_thread_id,
                    commit.agent_id.as_str(),
                    &inference.billing,
                )
                .await
            {
                tracing::error!(
                    agent_id = %commit.agent_id,
                    inference_id = %inference.billing.inference_id,
                    error = %error,
                    "model performance projection rejected an admitted Thread fact"
                );
            }
        }
        Ok(())
    }

    async fn await_durable(&self, thread_id: &ThreadId, revision: u64) -> Result<(), Self::Error> {
        self.writer
            .await_durable(thread_id.as_str(), revision)
            .await
    }

    fn pending_commit_count(&self) -> usize {
        self.writer.pending_commit_count()
    }

    async fn list_submissions(
        &self,
        thread_id: &ThreadId,
        offset: usize,
        limit: usize,
    ) -> Result<AgentSubmissionPage, Self::Error> {
        list_thread_submissions(&self.store, thread_id, offset, limit).await
    }
}

async fn list_thread_submissions(
    store: &StudioStore,
    thread_id: &ThreadId,
    offset: usize,
    limit: usize,
) -> Result<AgentSubmissionPage, PureError> {
    let thread_id = thread_id.to_string();
    let total = thread_submission::Entity::find()
        .filter(thread_submission::Column::ThreadId.eq(thread_id.clone()))
        .count(store.database())
        .await
        .map_err(store_error)?;
    let limit = limit.max(1);
    let rows = thread_submission::Entity::find()
        .filter(thread_submission::Column::ThreadId.eq(thread_id))
        .order_by_asc(thread_submission::Column::Ordinal)
        .offset(offset as u64)
        .limit(limit as u64)
        .all(store.database())
        .await
        .map_err(store_error)?;
    let items = rows
        .into_iter()
        .map(AgentSubmissionRecord::try_from)
        .collect::<Result<Vec<_>, PureError>>()?;
    let returned = items.len();
    let total_usize = total as usize;
    Ok(AgentSubmissionPage {
        items,
        offset,
        limit,
        total: total_usize,
        has_more: offset + returned < total_usize,
    })
}

/// 在调用方事务内应用一次 Thread commit；不负责 begin/commit/rollback。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ApplyCommitOutcome {
    Applied,
    AlreadyApplied,
    RevisionConflict { actual_revision: Option<u64> },
}

pub(super) async fn apply_state_commit(
    tx: &sea_orm::DatabaseTransaction,
    commit: &ThreadCommit,
) -> Result<ApplyCommitOutcome, PureError> {
    let thread_id = commit.agent_id.to_string();
    let payload_hash = thread_commit_payload_hash(commit)?;
    let Some(existing) = thread::Entity::find_by_id(thread_id.clone())
        .one(tx)
        .await
        .map_err(store_error)?
    else {
        return Err(store_error(format!(
            "Thread {thread_id} must exist before runtime registration"
        )));
    };
    let actual_revision = existing.runtime_revision.map(u64_from_i64).transpose()?;
    if actual_revision != commit.expected_revision {
        if actual_revision == Some(commit.next_state.snapshot.revision)
            && load_object::<ThreadCommitReceipt>(tx, &thread_id)
                .await
                .map_err(store_error)?
                .is_some_and(|receipt| {
                    receipt.revision == commit.next_state.snapshot.revision
                        && receipt.payload_hash == payload_hash
                })
        {
            return Ok(ApplyCommitOutcome::AlreadyApplied);
        }
        return Ok(ApplyCommitOutcome::RevisionConflict { actual_revision });
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
    active.state_json = Set(serde_json::to_string(&commit.next_state.snapshot.state)?);
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
    active.update(tx).await.map_err(store_error)?;

    persist_inputs(tx, &commit.next_state).await?;
    persist_state_turns(tx, &commit.next_state).await?;
    persist_thread_notifications(tx, commit).await?;
    persist_session_snapshot(tx, commit).await?;
    persist_inference_billing(tx, commit).await?;
    persist_submission(tx, commit).await?;
    put_object(
        tx,
        &thread_id,
        &ThreadCommitReceipt {
            revision: commit.next_state.snapshot.revision,
            payload_hash,
        },
        commit.next_state.snapshot.updated_at,
    )
    .await
    .map_err(store_error)?;
    Ok(ApplyCommitOutcome::Applied)
}

/// 在 persistence worker 内把 typed commit 转成稳定的 receipt payload 并计算 hash。
fn thread_commit_payload_hash(commit: &ThreadCommit) -> Result<String, PureError> {
    let persistence = match commit.persistence {
        pl_core::PersistenceClass::Coalescible => "coalescible",
        pl_core::PersistenceClass::Standard => "standard",
        pl_core::PersistenceClass::Settlement => "settlement",
    };
    let mutation = match &commit.mutation {
        pl_core::ThreadMutation::SnapshotAndQueue => serde_json::json!({
            "kind": "snapshotAndQueue"
        }),
        pl_core::ThreadMutation::ReplaceThread { thread_id } => serde_json::json!({
            "kind": "replaceThread",
            "threadId": thread_id.as_str(),
        }),
        pl_core::ThreadMutation::AppendTrace => serde_json::json!({
            "kind": "appendTrace"
        }),
        pl_core::ThreadMutation::AppendThreadNotifications { thread_id } => {
            serde_json::json!({
                "kind": "appendThreadNotifications",
                "threadId": thread_id.as_str(),
            })
        }
    };
    let context = commit.facts.context.as_ref().map(|context| match context {
        pl_core::ThreadContextMutation::Append { items } => serde_json::json!({
            "kind": "append",
            "items": items,
        }),
        pl_core::ThreadContextMutation::Replace { items } => serde_json::json!({
            "kind": "replace",
            "items": items,
        }),
    });
    let inference = commit.facts.inference.as_ref().map(|inference| {
        serde_json::json!({
            "billing": &inference.billing,
            "runtimeDelta": &inference.runtime_delta,
        })
    });
    let submission = commit.facts.submission.as_ref().map(|submission| {
        serde_json::json!({
            "report": &submission.report,
            "detail": &submission.detail,
            "createdAt": submission.created_at,
        })
    });
    let payload = serde_json::json!({
        "agentId": commit.agent_id.as_str(),
        "persistence": persistence,
        "expectedRevision": commit.expected_revision,
        "nextState": {
            "snapshot": &commit.next_state.snapshot,
            "session": {
                "metadata": &commit.next_state.session.metadata,
                "canonical": commit.next_state.session.session.snapshot(),
                "usage": &commit.next_state.session.usage,
                "billingByTurn": &commit.next_state.session.billing_by_turn,
                "lastContextTokens": commit.next_state.session.last_context_tokens,
                "traceSequence": commit.next_state.session.trace_sequence,
                "threadRevision": commit.next_state.session.thread_revision,
            },
            "pendingInputs": &commit.next_state.pending_inputs,
            "activeInput": &commit.next_state.active_input,
        },
        "facts": {
            "threadId": commit.facts.thread_id.as_str(),
            "turnId": commit.facts.turn_id.as_ref().map(|turn_id| turn_id.as_str()),
            "throughRevision": commit.facts.through_revision,
            "revision": commit.facts.revision,
            "notifications": &commit.facts.notifications,
            "turnTransition": &commit.facts.turn_transition,
            "context": context,
            "projectionSnapshot": &commit.facts.projection_snapshot,
            "runtimeEvents": &commit.facts.runtime_events,
            "traceEvents": &commit.facts.trace_events,
            "inference": inference,
            "submission": submission,
        },
        "mutation": mutation,
    });
    let encoded = serde_json::to_vec(&payload)?;
    Ok(pl_core::canonical_content_hash(&encoded))
}

/// 在同一事务内追加一条 durable 阶段提交记录（report_progress 触发）。
async fn persist_submission(
    tx: &sea_orm::DatabaseTransaction,
    commit: &ThreadCommit,
) -> Result<(), PureError> {
    let Some(submission) = commit.facts.submission.as_ref() else {
        return Ok(());
    };
    let thread_id = commit.agent_id.to_string();
    let next_ordinal = next_submission_ordinal(tx, &thread_id).await?;
    let stage = crate::studio::agent_host::events::progress_stage_label(submission.report.stage)
        .to_string();
    let active = thread_submission::ActiveModel {
        id: Set(crate::studio::ids::new_id("thread_submission")),
        thread_id: Set(thread_id),
        ordinal: Set(next_ordinal),
        stage: Set(stage),
        summary: Set(submission.report.summary.clone()),
        next_step: Set(submission.report.next_step.clone()),
        detail: Set(submission.detail.clone()),
        revision: Set(i64_from_u64(submission.report.revision)?),
        created_at: Set(submission.created_at),
    };
    active.insert(tx).await.map_err(store_error)?;
    Ok(())
}

async fn next_submission_ordinal(
    tx: &sea_orm::DatabaseTransaction,
    thread_id: &str,
) -> Result<i64, PureError> {
    let max = thread_submission::Entity::find()
        .filter(thread_submission::Column::ThreadId.eq(thread_id))
        .all(tx)
        .await
        .map_err(store_error)?
        .into_iter()
        .map(|model| model.ordinal)
        .max();
    Ok(max.map_or(0, |ordinal| ordinal.saturating_add(1)))
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
        upsert_input(tx, &thread_id, input).await?;
    }
    if let Some(input) = &state.active_input {
        live.insert(input.mail_id.clone());
        upsert_input(tx, &thread_id, input).await?;
    }
    for row in existing {
        if live.contains(&row.mail_id) || row.state_kind == "consumed" {
            continue;
        }
        let mut delivery_state: MailboxDeliveryState = serde_json::from_str(&row.state_json)?;
        if delivery_state.is_pending() {
            delivery_state = delivery_state
                .decide(MailboxCommand::Claim {
                    turn_id: TurnId::new(row.turn_id.clone())?,
                })
                .map_err(store_error)?
                .next_state;
        }
        let turn_id = delivery_state
            .turn_id()
            .cloned()
            .ok_or_else(|| store_error("claimed mailbox is missing its Turn identity"))?;
        let checkpoint_seq = delivery_state.checkpoint_seq().unwrap_or_default();
        let delivery_state = delivery_state
            .decide(MailboxCommand::Consume {
                turn_id,
                checkpoint_seq,
            })
            .map_err(store_error)?
            .next_state;
        let mut active = row.into_active_model();
        active.state_json = Set(serde_json::to_string(&delivery_state)?);
        active.update(tx).await.map_err(store_error)?;
    }
    Ok(())
}

async fn upsert_input(
    tx: &sea_orm::DatabaseTransaction,
    thread_id: &str,
    input: &DurableMailboxEnvelope,
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
    let active = thread_input::ActiveModel {
        id: Set(input.mail_id.clone()),
        thread_id: Set(thread_id.to_string()),
        mail_id: Set(input.mail_id.clone()),
        turn_id: Set(input.turn_id.to_string()),
        content: Set(input.payload.message.clone()),
        attachments_json: Set(serde_json::to_string(&input.payload.attachments)?),
        metadata_json: Set(serialize_input_metadata(input)?),
        presentation: Set(presentation_label(input.payload.presentation.clone()).to_string()),
        state_json: Set(serde_json::to_string(&input.delivery_state)?),
        queue_ordinal: Set(ordinal),
        queued_at: Set(existing
            .as_ref()
            .map_or(input.queued_at, |row| row.queued_at)),
        ..Default::default()
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

pub(super) fn u64_from_i64(value: i64) -> Result<u64, PureError> {
    u64::try_from(value).map_err(|error| store_error(error.to_string()))
}

pub(super) fn i64_from_u64(value: u64) -> Result<i64, PureError> {
    i64::try_from(value).map_err(|error| store_error(error.to_string()))
}

pub(super) fn store_error(error: impl std::fmt::Display) -> PureError {
    PureError::MemoryError(error.to_string())
}
