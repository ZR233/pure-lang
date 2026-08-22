//! 启动恢复:钉住集合计算与 Thread 快照/输入/会话恢复查询。

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use pl_core::{
    AgentIdentity, AgentRoleId, AgentSession, AgentSnapshot, AgentState, AgentTurnOutcome,
    DurableMailboxEnvelope, RestoredAgentRuntime, RestoredThreadSnapshot, ThreadActorState,
    ThreadContextState, ThreadId,
};
use pl_protocol::{PureError, ThreadItem, ThreadItemState, ThreadSnapshot, Turn};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};

use crate::studio::entity::{interaction, item, thread, thread_input, turn};

use super::StudioAgentRepository;
use super::billing::{aggregate_billing_usage, restore_billing, runtime_from_context};
use super::context::{SessionSnapshotAuditError, audit_session_snapshot, restore_session_snapshot};
use super::labels::agent_state_kind;
use super::projection::latest_turn;
use super::{StudioSessionRecoveryFailure, anyhow_into, store_error, u64_from_i64};

impl StudioAgentRepository {
    /// 钉住集合：pending input、pending Interaction、活动 Turn、活动 Task root、
    /// pending planner wake root 与 pending executor continuation agent。
    pub(super) async fn pinned_thread_ids(&self) -> Result<BTreeSet<String>, PureError> {
        let database = self.store.database();
        let mut ids = BTreeSet::new();
        ids.extend(
            thread_input::Entity::find()
                .filter(thread_input::Column::StateKind.ne("consumed"))
                .all(database)
                .await
                .map_err(store_error)?
                .into_iter()
                .map(|row| row.thread_id),
        );
        ids.extend(
            interaction::Entity::find()
                .filter(interaction::Column::StateKind.eq("pending"))
                .all(database)
                .await
                .map_err(store_error)?
                .into_iter()
                .map(|row| row.thread_id),
        );
        ids.extend(
            turn::Entity::find()
                .filter(turn::Column::StateKind.is_in(["queued", "running"]))
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
            ids.insert(run.root_thread_id.clone());
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
    pub(super) async fn ancestor_parents(
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
    pub(super) async fn restore_model(
        &self,
        model: thread::Model,
        parents: &BTreeMap<String, Option<String>>,
    ) -> Result<RestoredAgentRuntime, PureError> {
        let thread_id = ThreadId::new(model.id.clone())?;
        let (pending_inputs, active_input) = self.restore_inputs(thread_id.as_str()).await?;
        let last_turn = latest_turn(&self.store, thread_id.as_str(), false)
            .await?
            .map(AgentTurnOutcome::try_from)
            .transpose()?;
        let state: AgentState = serde_json::from_str(&model.state_json)?;
        if agent_state_kind(&state) != model.state_kind {
            return Err(store_error(format!(
                "Agent state discriminator mismatch: JSON is {}, generated column is {}",
                agent_state_kind(&state),
                model.state_kind
            )));
        }
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
            state,
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

    pub(super) async fn session_recovery_failures(
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

    pub(super) async fn restore_inputs(
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
            .filter(thread_input::Column::StateKind.ne("consumed"))
            .order_by_asc(thread_input::Column::QueueOrdinal)
            .all(self.store.database())
            .await
            .map_err(store_error)?;
        let mut pending = VecDeque::new();
        let mut active = None;
        for row in rows {
            let is_active = row.state_kind == "claimed";
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

    pub(super) async fn restore_session(
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

    pub(super) async fn restore_thread_snapshot(
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
            .map(ThreadItem::try_from)
            .collect::<Result<Vec<ThreadItem>, PureError>>()?
            .into_iter()
            .filter(|item| !matches!(item.state(), ThreadItemState::ContextCompaction(_)))
            .collect();
        let active_turn = turn::Entity::find()
            .filter(turn::Column::ThreadId.eq(thread_id.clone()))
            .filter(turn::Column::StateKind.is_in(["queued", "running"]))
            .order_by_desc(turn::Column::Ordinal)
            .one(self.store.database())
            .await
            .map_err(store_error)?
            .map(Turn::try_from)
            .transpose()?;
        let interactions = interaction::Entity::find()
            .filter(interaction::Column::ThreadId.eq(thread_id.clone()))
            .filter(interaction::Column::StateKind.eq("pending"))
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
