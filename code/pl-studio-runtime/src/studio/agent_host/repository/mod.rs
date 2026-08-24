use std::collections::BTreeSet;

use pl_core::{
    AgentSubmissionPage, AgentSubmissionRecord, DurableMailboxEnvelope, MailboxCommand,
    MailboxDeliveryState, RestoredAgentRuntime, ThreadActorState, ThreadCommit, ThreadId,
    ThreadRepository, TurnId,
};
use pl_protocol::ThreadSnapshot;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, IntoActiveModel, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect,
};

use crate::PureError;
use crate::studio::StudioStore;
use crate::studio::entity::{thread, thread_input, thread_submission};

mod billing;
mod context;
mod conversion;
pub(super) mod labels;
mod write_behind;

use billing::persist_inference_billing;
use context::{
    SessionSnapshotAuditError, audit_session_snapshot, persist_session_snapshot,
    serialize_thread_metadata,
};
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
    pub(in crate::studio) fn new(store: StudioStore) -> Self {
        let writer = ThreadWriteBehindWriter::new(store.clone());
        Self::with_writer(store, writer)
    }

    pub(in crate::studio) fn with_writer(
        store: StudioStore,
        writer: ThreadWriteBehindWriter,
    ) -> Self {
        Self { writer, store }
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

    async fn commit(&self, commit: ThreadCommit) -> Result<(), Self::Error> {
        self.writer.enqueue(commit).await
    }

    async fn flush_pending(&self, thread_id: Option<&ThreadId>) -> Result<(), Self::Error> {
        let Some(thread_id) = thread_id else {
            return self.writer.flush().await;
        };
        let Some(revision) = self.writer.latest_queued_revision(thread_id.as_str()) else {
            return Ok(());
        };
        self.writer
            .await_durable(thread_id.as_str(), revision)
            .await
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
    RevisionConflict { actual_revision: Option<u64> },
}

pub(super) async fn apply_state_commit(
    tx: &sea_orm::DatabaseTransaction,
    commit: &ThreadCommit,
) -> Result<ApplyCommitOutcome, PureError> {
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
    Ok(ApplyCommitOutcome::Applied)
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

fn anyhow_into(error: anyhow::Error) -> PureError {
    PureError::MemoryError(error.to_string())
}

/// write-behind writer 与 repository 的事务级测试支撑。
#[cfg(test)]
pub(super) mod test_support {
    use std::collections::VecDeque;

    use pl_core::{
        AgentIdentity, AgentRoleId, AgentSnapshot, AgentState, DurableCommitFacts,
        PersistenceClass, ThreadActorState, ThreadCommit, ThreadContextState, ThreadId,
        ThreadMutation,
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
        persistence: PersistenceClass,
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
                state: AgentState::idle(),
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
            persistence,
            expected_revision: None,
            facts: DurableCommitFacts::from_state(&state, Vec::new(), Vec::new(), None, None),
            next_state: state,
            mutation: ThreadMutation::SnapshotAndQueue,
        }
    }
}
