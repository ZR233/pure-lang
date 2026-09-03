//! 启动恢复:钉住集合计算与 Thread 快照/输入/会话恢复查询。

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use pl_core::{
    AgentCommand, AgentFaultClassification, AgentIdentity, AgentRecoveryTarget, AgentRoleId,
    AgentSession, AgentSnapshot, AgentState, AgentStateTransition, AgentTurnOutcome,
    DurableMailboxEnvelope, FaultedAgentState, RestoredAgentRuntime, RestoredThreadSnapshot,
    ThreadActorState, ThreadContextState, ThreadId,
};
use pl_protocol::{
    PureError, ThreadItem, ThreadItemState, ThreadRuntimeSnapshot, ThreadRuntimeUsage,
    ThreadSnapshot, Turn,
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};

use crate::studio::entity::{interaction, item, thread, thread_input, turn};

use super::StudioAgentRepository;
use super::billing::{aggregate_billing_usage, restore_billing, runtime_from_context};
use super::context::{SessionSnapshotAuditError, audit_session_snapshot, restore_session_snapshot};
use super::labels::agent_state_kind;
use super::projection::latest_turn;
use super::{StudioSessionRecoveryFailure, store_error, u64_from_i64};

const HOT_TIMELINE_ITEM_LIMIT: u64 = 400;

impl StudioAgentRepository {
    /// 钉住集合：pending input、pending Interaction 与活动 Turn。
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
        let (pending_inputs, mut active_input) = self.restore_inputs(thread_id.as_str()).await?;
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
        let was_faulted = matches!(state, AgentState::Faulted(_));
        let state = recover_validated_fault(state, last_turn.as_ref())?;
        if was_faulted && state.is_idle() {
            // Faulted 提交已经把旧 Turn 记录为失败；恢复不得复活其 claimed 输入。
            // 下一次热提交会把该旧输入规范化为 consumed。
            active_input = None;
        }
        let durable_revision =
            u64_from_i64(model.runtime_revision.ok_or_else(|| {
                store_error(format!("Thread {} actor is not registered", model.id))
            })?)?;
        self.writer
            .seed_durable_revision(thread_id.as_str(), durable_revision);
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
            revision: durable_revision,
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
            match self.audit_thread_recovery_payloads(&model.id).await {
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

    /// 冷激活前严格审计 session 与 wire-v7 Skill item；不得在读取时修复旧 payload。
    pub(super) async fn audit_thread_recovery_payloads(
        &self,
        thread_id: &str,
    ) -> Result<(), SessionSnapshotAuditError> {
        audit_session_snapshot(&self.store, thread_id).await?;
        let skill_items = item::Entity::find()
            .filter(item::Column::ThreadId.eq(thread_id))
            .filter(item::Column::StateKind.eq("skill"))
            .order_by_asc(item::Column::Ordinal)
            .all(self.store.database())
            .await
            .map_err(|error| SessionSnapshotAuditError::Fatal(store_error(error)))?;
        for row in skill_items {
            ThreadItem::try_from(row).map_err(SessionSnapshotAuditError::Corrupt)?;
        }
        Ok(())
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
        let mut item_rows = item::Entity::find()
            .filter(item::Column::ThreadId.eq(thread_id.clone()))
            .order_by_desc(item::Column::Ordinal)
            .limit(HOT_TIMELINE_ITEM_LIMIT)
            .all(self.store.database())
            .await
            .map_err(store_error)?;
        if item_rows.len() == HOT_TIMELINE_ITEM_LIMIT as usize
            && let Some(cutoff_turn_id) = item_rows.last().map(|row| row.turn_id.clone())
        {
            let existing_ids = item_rows
                .iter()
                .map(|row| row.id.clone())
                .collect::<BTreeSet<_>>();
            item_rows.extend(
                item::Entity::find()
                    .filter(item::Column::ThreadId.eq(thread_id.clone()))
                    .filter(item::Column::TurnId.eq(cutoff_turn_id))
                    .order_by_asc(item::Column::Ordinal)
                    .all(self.store.database())
                    .await
                    .map_err(store_error)?
                    .into_iter()
                    .filter(|row| !existing_ids.contains(&row.id)),
            );
        }
        item_rows.sort_by_key(|row| row.ordinal);
        let items: Vec<ThreadItem> = item_rows
            .into_iter()
            .map(ThreadItem::try_from)
            .collect::<Result<Vec<ThreadItem>, PureError>>()?
            .into_iter()
            .filter(|item| !matches!(item.state(), ThreadItemState::ContextCompaction(_)))
            .collect();
        // active skill 属于 working runtime，而不是 Timeline 窗口。它可能早于最近
        // 400 项，因此单独按 typed Skill item 恢复，但不把旧 item 混入 GUI 热窗口。
        let skill_items = item::Entity::find()
            .filter(item::Column::ThreadId.eq(thread_id.clone()))
            .filter(item::Column::StateKind.eq("skill"))
            .order_by_asc(item::Column::Ordinal)
            .all(self.store.database())
            .await
            .map_err(store_error)?
            .into_iter()
            .map(ThreadItem::try_from)
            .collect::<Result<Vec<ThreadItem>, PureError>>()?;
        let active_skills = active_skills_from_items(&skill_items);
        let latest_activation_at = skill_items
            .iter()
            .filter_map(|item| match item.state() {
                ThreadItemState::Skill(skill) => Some(skill.activation().activated_at),
                ThreadItemState::Text(_)
                | ThreadItemState::Thinking(_)
                | ThreadItemState::Tool(_)
                | ThreadItemState::Agent(_)
                | ThreadItemState::Turn(_)
                | ThreadItemState::Inference(_)
                | ThreadItemState::File(_)
                | ThreadItemState::ContextCompaction(_) => None,
            })
            .max()
            .unwrap_or(0);
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
        let mut runtime = runtime_from_context(&thread_id, context);
        if !active_skills.is_empty() {
            let runtime = runtime
                .get_or_insert_with(|| empty_restored_runtime(&thread_id, latest_activation_at));
            runtime.active_skills = active_skills;
            runtime.updated_at = runtime.updated_at.max(latest_activation_at);
        }
        Ok(RestoredThreadSnapshot {
            snapshot: ThreadSnapshot {
                schema_version: pl_protocol::THREAD_SCHEMA_VERSION,
                revision: u64_from_i64(model.revision)?,
                thread: model.try_into()?,
                active_turn,
                items,
                interactions,
                runtime,
            },
        })
    }
}

pub(super) fn active_skills_from_items(items: &[ThreadItem]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    items
        .iter()
        .filter_map(|item| match item.state() {
            ThreadItemState::Skill(skill) => Some(skill.activation().name.clone()),
            ThreadItemState::Text(_)
            | ThreadItemState::Thinking(_)
            | ThreadItemState::Tool(_)
            | ThreadItemState::Agent(_)
            | ThreadItemState::Turn(_)
            | ThreadItemState::Inference(_)
            | ThreadItemState::File(_)
            | ThreadItemState::ContextCompaction(_) => None,
        })
        .filter(|name| seen.insert(name.clone()))
        .collect()
}

fn empty_restored_runtime(thread_id: &str, updated_at: i64) -> ThreadRuntimeSnapshot {
    ThreadRuntimeSnapshot {
        thread_id: thread_id.to_string(),
        usage: ThreadRuntimeUsage {
            model: String::new(),
            context_window: None,
            latest_context_tokens: 0,
            prompt_tokens: 0,
            completion_tokens: 0,
            cached_prompt_tokens: 0,
            cache_write_tokens: 0,
            cache_miss_tokens: 0,
            reasoning_tokens: 0,
            inference_count: 0,
            total_tokens: 0,
            cache_hit_rate: None,
            estimated_costs: Vec::new(),
            estimated_cache_savings: Vec::new(),
            has_unpriced_usage: false,
            prompt_generation: None,
            prompt_cache_policy: None,
            prefix_changed_reason: None,
            updated_at,
        },
        turn_completion_tokens: 0,
        turn_decode_millis: 0,
        todo: None,
        active_skills: Vec::new(),
        workflow: None,
        active_mcp_servers: Vec::new(),
        active_lsp_servers: Vec::new(),
        progress: None,
        mcp_health: None,
        updated_at,
    }
}

const LEGACY_REASONING_CHUNK_FAULT: &str = "chunk index skipped an earlier chunk";

/// 只兼容已经确认由旧 reasoning 分块编号回归造成的历史故障。
///
/// 调用本函数前，恢复入口已经完成 session hash 与 transcript 校验；实时投影仍保留
/// 严格跳号检查，其他 Faulted 状态也继续 fail-closed。
fn recover_validated_fault(
    state: AgentState,
    last_turn: Option<&AgentTurnOutcome>,
) -> Result<AgentState, PureError> {
    let AgentState::Faulted(faulted) = &state else {
        return Ok(state);
    };
    let known_legacy_fault = faulted
        .error()
        .message
        .contains(LEGACY_REASONING_CHUNK_FAULT)
        && last_turn
            .and_then(|turn| turn.outcome.failure())
            .is_some_and(|failure| failure.message.contains(LEGACY_REASONING_CHUNK_FAULT));
    if !faulted.classification().is_recoverable() && !known_legacy_fault {
        return Ok(state);
    }
    let state = if known_legacy_fault
        && faulted.classification() == AgentFaultClassification::LegacyUnknown
    {
        AgentState::Faulted(FaultedAgentState::classified(
            faulted.error().clone(),
            faulted.turn_id().cloned(),
            AgentFaultClassification::RecoverableProtocol,
        ))
    } else {
        state
    };
    tracing::warn!("recovering a validated typed Agent fault as an idle in-memory agent");
    state
        .decide(AgentCommand::RecoverFaulted {
            target: AgentRecoveryTarget::Idle,
        })
        .map(|decision| decision.next_state)
        .map_err(store_error)
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

#[cfg(test)]
mod tests {
    use pl_core::{AgentTurnOutcome, FaultedAgentState, ThreadId, TurnId};
    use pl_protocol::TokenUsage;
    use pl_protocol::{StateError, TurnFailure, TurnFailureCategory, TurnOutcome};

    use super::*;

    #[test]
    fn validated_legacy_reasoning_chunk_fault_recovers_to_idle() {
        let outcome = AgentTurnOutcome {
            turn_id: TurnId::new("turn-1").unwrap(),
            thread_id: ThreadId::new("thread-1").unwrap(),
            outcome: TurnOutcome::failed(TurnFailure::permanent(
                TurnFailureCategory::Internal,
                format!("projection failed: {LEGACY_REASONING_CHUNK_FAULT}"),
            )),
            usage: TokenUsage::default(),
            started_at: Some(1),
            finished_at: 2,
        };
        let state = AgentState::Faulted(FaultedAgentState::new(
            StateError {
                code: "agentRuntimeFault".to_string(),
                message: format!("thread events failed: {LEGACY_REASONING_CHUNK_FAULT}"),
                retryable: false,
            },
            Some(TurnId::new("turn-1").unwrap()),
        ));

        let recovered = recover_validated_fault(state, Some(&outcome)).unwrap();

        assert!(recovered.is_idle());
    }

    #[test]
    fn unrelated_fault_remains_faulted() {
        let state = AgentState::Faulted(FaultedAgentState::new(
            StateError {
                code: "agentRuntimeFault".to_string(),
                message: "aggregate validation failed".to_string(),
                retryable: false,
            },
            None,
        ));

        let recovered = recover_validated_fault(state, None).unwrap();

        assert!(matches!(recovered, AgentState::Faulted(_)));
    }

    #[test]
    fn validated_typed_runtime_fault_recovers_without_matching_legacy_text() {
        let state = AgentState::Faulted(FaultedAgentState::classified(
            StateError {
                code: "agentRuntimeRecoverable".to_string(),
                message: "runtime loop failed after a verified commit".to_string(),
                retryable: false,
            },
            None,
            AgentFaultClassification::RecoverableRuntime,
        ));

        let recovered = recover_validated_fault(state, None).unwrap();

        assert!(recovered.is_idle());
    }
}
