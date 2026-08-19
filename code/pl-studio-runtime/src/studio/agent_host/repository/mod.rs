use std::collections::{BTreeMap, BTreeSet, VecDeque};

use pl_core::{
    ActiveKind, AgentActivityState, AgentIdentity, AgentRoleId, AgentSession, AgentSnapshot,
    AgentSubmissionPage, AgentSubmissionRecord, AgentTurnOutcome, DurableMailboxEnvelope,
    MailboxDeliveryState, RestoredAgentRuntime, RestoredThreadSnapshot, ThreadActorState,
    ThreadCommit, ThreadCommitOutcome, ThreadContextState, ThreadId, ThreadRepository, TurnId,
};
use pl_protocol::{ThreadItem, ThreadItemContent, ThreadNotification, ThreadSnapshot, Turn};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, IntoActiveModel, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect,
};

use crate::PureError;
use crate::studio::StudioStore;
use crate::studio::entity::{interaction, item, thread, thread_input, thread_submission, turn};

mod billing;
mod context;
mod conversion;
mod labels;
mod write_behind;

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
    item_status_label, lifecycle_from_status, outcome_columns, presentation_label,
    thread_status_label, turn_state_columns,
};

use write_behind::ThreadWriteBehindWriter;

/// Studio 单库对 canonical Thread 状态的 write-behind repository。
///
/// commit 只进入 [`ThreadWriteBehindWriter`] 队列，由后台批量事务落库；
/// 内存 actor state 是唯一权威实例。
#[derive(Clone)]
pub(in crate::studio) struct StudioAgentRepository {
    store: StudioStore,
    writer: ThreadWriteBehindWriter,
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
        Self {
            writer: ThreadWriteBehindWriter::new(store.clone()),
            store,
        }
    }

    /// 只读用途构造（restore_thread / read_thread_snapshot 等）。
    ///
    /// writer 不会被使用；禁止在该实例上调用 commit/flush。
    pub(in crate::studio) fn for_reads(store: StudioStore) -> Self {
        Self::new(store)
    }

    /// write-behind writer 句柄；关机排空与进度查询使用。
    pub(in crate::studio) fn writer(&self) -> &ThreadWriteBehindWriter {
        &self.writer
    }

    pub(in crate::studio) async fn read_thread_snapshot(
        &self,
        thread_id: &str,
    ) -> Result<Option<ThreadSnapshot>, PureError> {
        let Some(model) = thread::Entity::find_by_id(thread_id.to_string())
            .one(self.store.database())
            .await
            .map_err(store_error)?
        else {
            return Ok(None);
        };
        if model.archived != 0 {
            return Ok(None);
        }
        if model.runtime_revision.is_none() {
            return Ok(Some(ThreadSnapshot {
                schema_version: pl_protocol::THREAD_SCHEMA_VERSION,
                revision: u64_from_i64(model.revision)?,
                thread: model.try_into()?,
                active_turn: None,
                items: Vec::new(),
                interactions: Vec::new(),
                runtime: None,
            }));
        }
        let context = self.restore_session(&model).await?;
        Ok(Some(
            self.restore_thread_snapshot(model, &context)
                .await?
                .snapshot,
        ))
    }

    /// 钉住集合：pending input、pending Interaction、活动 Turn、活动 Task root、
    /// pending planner wake root 与 pending executor continuation agent。
    async fn pinned_thread_ids(&self) -> Result<BTreeSet<String>, PureError> {
        let database = self.store.database();
        let mut ids = BTreeSet::new();
        ids.extend(
            thread_input::Entity::find()
                .filter(thread_input::Column::State.ne("consumed"))
                .all(database)
                .await
                .map_err(store_error)?
                .into_iter()
                .map(|row| row.thread_id),
        );
        ids.extend(
            interaction::Entity::find()
                .filter(interaction::Column::Status.eq("pending"))
                .all(database)
                .await
                .map_err(store_error)?
                .into_iter()
                .map(|row| row.thread_id),
        );
        ids.extend(
            turn::Entity::find()
                .filter(turn::Column::Status.is_in(["queued", "inProgress"]))
                .all(database)
                .await
                .map_err(store_error)?
                .into_iter()
                .map(|row| row.thread_id),
        );
        for run in self
            .store
            .list_active_task_runs()
            .await
            .map_err(anyhow_into)?
        {
            ids.insert(run.root_thread_id);
        }
        for wake in self
            .store
            .list_pending_task_planner_wakes()
            .await
            .map_err(anyhow_into)?
        {
            ids.insert(wake.root_thread_id.clone());
        }
        for continuation in self
            .store
            .list_pending_executor_continuations()
            .await
            .map_err(anyhow_into)?
        {
            ids.insert(continuation.agent_id.clone());
        }
        Ok(ids)
    }

    /// 为 depth 计算构建 parent 映射；不钉住的祖先只进入映射，不恢复 actor。
    async fn ancestor_parents(
        &self,
        models: &[thread::Model],
    ) -> Result<BTreeMap<String, Option<String>>, PureError> {
        let mut parents: BTreeMap<String, Option<String>> = models
            .iter()
            .map(|model| (model.id.clone(), model.parent_thread_id.clone()))
            .collect();
        for model in models {
            let mut cursor = model.parent_thread_id.clone();
            let mut remaining = models.len() + 64;
            while let Some(parent_id) = cursor {
                if parents.contains_key(&parent_id) {
                    break;
                }
                if remaining == 0 {
                    return Err(store_error("Thread parent graph contains a cycle"));
                }
                remaining -= 1;
                let parent = thread::Entity::find_by_id(parent_id.clone())
                    .one(self.store.database())
                    .await
                    .map_err(store_error)?
                    .ok_or_else(|| store_error(format!("Thread parent {parent_id} is missing")))?;
                cursor = parent.parent_thread_id.clone();
                parents.insert(parent.id.clone(), parent.parent_thread_id.clone());
            }
        }
        Ok(parents)
    }

    /// 把单个 thread 行恢复成驻留 actor 状态。
    async fn restore_model(
        &self,
        model: thread::Model,
        parents: &BTreeMap<String, Option<String>>,
    ) -> Result<RestoredAgentRuntime, PureError> {
        let thread_id = ThreadId::new(model.id.clone())?;
        let (pending_inputs, active_input) = self.restore_inputs(thread_id.as_str()).await?;
        let active_turn = latest_turn(&self.store, thread_id.as_str(), true).await?;
        let last_turn = latest_turn(&self.store, thread_id.as_str(), false)
            .await?
            .map(AgentTurnOutcome::try_from)
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
                depth: thread_depth(&model.id, parents)?,
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
        Ok(RestoredAgentRuntime {
            state: ThreadActorState {
                snapshot,
                session,
                pending_inputs,
                active_input,
            },
            thread_snapshot: Some(thread_snapshot),
        })
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
            let is_active = row.state == "active";
            let input = row.try_into()?;
            if is_active {
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
            .map(Turn::try_from)
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
                thread: model.try_into()?,
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

    /// 只恢复启动钉住集合：存在 pending input、pending Interaction、活动 Turn
    /// 或被活动 Task/唤醒/续轮引用的 Thread。其余 Thread 惰性驻留。
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
        if let Err(SessionSnapshotAuditError::Corrupt(_)) =
            audit_session_snapshot(&self.store, &model.id).await
        {
            tracing::warn!(
                agent_thread_id = %model.id,
                " refusing to lazily restore a thread with a corrupt durable session"
            );
            return Ok(None);
        }
        let parents = self.ancestor_parents(std::slice::from_ref(&model)).await?;
        Ok(Some(self.restore_model(model, &parents).await?))
    }

    async fn commit(&self, commit: ThreadCommit) -> Result<ThreadCommitOutcome, Self::Error> {
        self.writer.enqueue(commit).await
    }

    async fn flush_pending(&self, _thread_id: Option<&ThreadId>) -> Result<(), Self::Error> {
        self.writer.flush().await
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
pub(super) async fn apply_state_commit(
    tx: &sea_orm::DatabaseTransaction,
    commit: &ThreadCommit,
) -> Result<ThreadCommitOutcome, PureError> {
    let thread_id = commit.agent_id.to_string();
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
    active.update(tx).await.map_err(store_error)?;

    persist_inputs(tx, &commit.next_state).await?;
    persist_state_turns(tx, &commit.next_state).await?;
    persist_thread_notifications(tx, commit).await?;
    persist_session_snapshot(tx, commit).await?;
    persist_inference_billing(tx, commit).await?;
    persist_submission(tx, commit).await?;
    Ok(ThreadCommitOutcome::Applied)
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
        content: Set(input.payload.message.clone()),
        metadata_json: Set(serialize_input_metadata(input)?),
        presentation: Set(presentation_label(input.payload.presentation.clone()).to_string()),
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

const RUNTIME_INPUT_METADATA_KEY: &str = "$plAgentRuntime";
const INPUT_METADATA_PAYLOAD_KEY: &str = "payload";
const INPUT_METADATA_BUDGET_ACTION_KEY: &str = "budgetAction";

fn serialize_input_metadata(input: &DurableMailboxEnvelope) -> Result<String, PureError> {
    if input.queue_coalescing_key.is_none()
        && input.budget_action == pl_core::MailboxBudgetAction::Preserve
    {
        return Ok(serde_json::to_string(&input.payload.metadata)?);
    }
    let mut runtime = serde_json::Map::new();
    if let Some(key) = input.queue_coalescing_key.as_deref() {
        runtime.insert(
            "queueCoalescingKey".to_string(),
            serde_json::Value::String(key.to_string()),
        );
    }
    if input.budget_action != pl_core::MailboxBudgetAction::Preserve {
        runtime.insert(
            INPUT_METADATA_BUDGET_ACTION_KEY.to_string(),
            serde_json::Value::String(input.budget_action.as_str().to_string()),
        );
    }
    let value = serde_json::json!({
        RUNTIME_INPUT_METADATA_KEY: runtime,
        INPUT_METADATA_PAYLOAD_KEY: input.payload.metadata,
    });
    Ok(serde_json::to_string(&value)?)
}

fn deserialize_input_metadata(
    input: &str,
) -> Result<
    (
        serde_json::Value,
        Option<String>,
        pl_core::MailboxBudgetAction,
    ),
    PureError,
> {
    let mut value: serde_json::Value = serde_json::from_str(input)?;
    let Some(object) = value.as_object_mut() else {
        return Ok((value, None, pl_core::MailboxBudgetAction::Preserve));
    };
    let Some(runtime) = object.get(RUNTIME_INPUT_METADATA_KEY) else {
        return Ok((value, None, pl_core::MailboxBudgetAction::Preserve));
    };
    let key = runtime
        .get("queueCoalescingKey")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned);
    let budget_action = match runtime
        .get(INPUT_METADATA_BUDGET_ACTION_KEY)
        .and_then(serde_json::Value::as_str)
    {
        Some(value) => pl_core::MailboxBudgetAction::from_persisted_str(value)
            .ok_or_else(|| store_error(format!("unknown mailbox budget action {value}")))?,
        None => pl_core::MailboxBudgetAction::Preserve,
    };
    let payload = object
        .remove(INPUT_METADATA_PAYLOAD_KEY)
        .unwrap_or(serde_json::Value::Null);
    Ok((payload, key, budget_action))
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
                metadata: Some(&input.payload.metadata),
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
            failure: value.failure.as_ref(),
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
    // ordinal 是内存权威事实：由 ThreadEventBus 首次应用时一次性分配，
    // DB 只原样落库（含既有行），不再派生顺序事实。
    let active = item::ActiveModel {
        id: Set(value.id.clone()),
        thread_id: Set(value.thread_id.clone()),
        turn_id: Set(value.turn_id.clone()),
        ordinal: Set(i64_from_u64(value.ordinal)?),
        revision: Set(i64_from_u64(value.revision)?),
        item_kind: Set(item_kind_label(&value.content).to_string()),
        status: Set(item_status_label(value.status).to_string()),
        payload_json: Set(serde_json::to_string(value)?),
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
        // 老数据兼容：waitingInteraction phase 的 Turn 在 TryFrom 实现里已降级为
        // completed，不会进入 active turn 查询；这里不再映射该 phase。
        Some(turn) => match turn.phase.as_deref() {
            Some("runningTool") => AgentActivityState::Active(ActiveKind::WaitingTool),
            _ => AgentActivityState::Active(ActiveKind::Running),
        },
        None if !pending.is_empty() => AgentActivityState::Queued,
        None => AgentActivityState::Idle,
    }
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

fn anyhow_into(error: anyhow::Error) -> PureError {
    PureError::MemoryError(error.to_string())
}

/// write-behind writer 与 repository 的事务级测试支撑。
#[cfg(test)]
pub(super) mod test_support {
    use std::collections::VecDeque;

    use pl_core::{
        AgentActivityState, AgentIdentity, AgentLifecycleState, AgentRoleId, AgentSnapshot,
        CommitDurability, DurableCommitFacts, ThreadActorState, ThreadCommit, ThreadContextState,
        ThreadId, ThreadMutation,
    };

    use super::StudioStore;
    use crate::config::StudioMode;

    /// 建立内存库中的 project + thread 行，返回 thread id。
    pub(super) async fn seed_thread(store: &StudioStore, title: &str) -> String {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let workspace = std::env::temp_dir().join(format!("pure-writer-support-{unique}-{title}"));
        let project = store.upsert_project(&workspace).await.expect("project");
        let thread = store
            .create_thread(&project.id, title, StudioMode::Simple)
            .await
            .expect("thread");
        thread.id
    }

    /// 构造首次注册（expected_revision=None）的最小 ThreadCommit。
    pub(super) fn writer_test_commit(
        thread_id: &str,
        durability: CommitDurability,
    ) -> ThreadCommit {
        let thread_id = ThreadId::new(thread_id).expect("thread id");
        let state = ThreadActorState {
            snapshot: AgentSnapshot {
                identity: AgentIdentity {
                    id: thread_id.clone(),
                    parent_id: None,
                    role: AgentRoleId::new("executor").expect("role"),
                    depth: 0,
                },
                lifecycle: AgentLifecycleState::Active,
                activity: AgentActivityState::Idle,
                active_turn_id: None,
                pending_inputs: 0,
                progress: None,
                last_turn: None,
                revision: 1,
                event_sequence: 1,
                updated_at: 1,
            },
            session: ThreadContextState::empty(),
            pending_inputs: VecDeque::new(),
            active_input: None,
        };
        ThreadCommit {
            agent_id: thread_id.clone(),
            durability,
            expected_revision: None,
            facts: DurableCommitFacts::from_state(&state, Vec::new(), Vec::new(), None, None),
            next_state: state,
            mutation: ThreadMutation::SnapshotAndQueue,
        }
    }
}

#[cfg(test)]
mod outcome_tests {
    use super::*;
    use pl_core::{MailboxPresentation, TurnOutcomeKind};

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
        let outcome = AgentTurnOutcome::try_from(turn::Model {
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
            payload: pl_core::MailboxInputPayload {
                message: "wake".to_string(),
                presentation: MailboxPresentation::Hidden,
                metadata: serde_json::json!({"kind": "taskWake"}),
            },
            queue_coalescing_key: Some("task-run:wakes".to_string()),
            budget_action: pl_core::MailboxBudgetAction::Preserve,
            delivery_state: MailboxDeliveryState::Pending,
            queued_at: 1,
        };

        let stored = serialize_input_metadata(&input).unwrap();
        let (metadata, key, budget_action) = deserialize_input_metadata(&stored).unwrap();

        assert_eq!(metadata, input.payload.metadata);
        assert_eq!(key, input.queue_coalescing_key);
        assert_eq!(budget_action, pl_core::MailboxBudgetAction::Preserve);
    }

    #[test]
    fn input_metadata_round_trips_budget_refresh_without_queue_key() {
        let input = DurableMailboxEnvelope {
            mail_id: "mail:refresh".to_string(),
            turn_id: TurnId::new("turn-refresh").unwrap(),
            thread_id: ThreadId::new("thread-refresh").unwrap(),
            payload: pl_core::MailboxInputPayload {
                message: "continue".to_string(),
                presentation: MailboxPresentation::Hidden,
                metadata: serde_json::json!({"kind": "plannerMessage"}),
            },
            queue_coalescing_key: None,
            budget_action: pl_core::MailboxBudgetAction::Refresh,
            delivery_state: MailboxDeliveryState::Pending,
            queued_at: 1,
        };

        let stored = serialize_input_metadata(&input).unwrap();
        let (metadata, key, budget_action) = deserialize_input_metadata(&stored).unwrap();

        assert_eq!(metadata, input.payload.metadata);
        assert_eq!(key, None);
        assert_eq!(budget_action, pl_core::MailboxBudgetAction::Refresh);
    }

    #[test]
    fn legacy_input_metadata_remains_unwrapped() {
        let stored = r#"{"kind":"legacy"}"#;
        let (metadata, key, budget_action) = deserialize_input_metadata(stored).unwrap();

        assert_eq!(metadata, serde_json::json!({"kind": "legacy"}));
        assert_eq!(key, None);
        assert_eq!(budget_action, pl_core::MailboxBudgetAction::Preserve);
    }
}
