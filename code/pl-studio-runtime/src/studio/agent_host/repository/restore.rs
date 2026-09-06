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
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};

use crate::studio::entity::{interaction, item, thread, thread_input, turn};

use super::StudioAgentRepository;
use super::billing::{aggregate_billing_usage, restore_billing, runtime_from_context};
use super::context::{SessionSnapshotAuditError, audit_session_snapshot, restore_session_snapshot};
use super::labels::agent_state_kind;
use super::projection::latest_turn;
use super::{StudioSessionRecoveryFailure, store_error, u64_from_i64};

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
        let idle_agents = thread::Entity::find()
            .filter(thread::Column::RuntimeRevision.is_not_null())
            .filter(thread::Column::StateKind.eq("idle"))
            .all(database)
            .await
            .map_err(store_error)?;
        for model in idle_agents {
            let state: AgentState = serde_json::from_str(&model.state_json)?;
            if state.is_budget_paused() {
                ids.insert(model.id);
            }
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
        let submissions = super::submissions::list_thread_submissions(
            &self.store,
            &ThreadId::new(model.id.clone())?,
            0,
            i64::MAX as usize,
        )
        .await?
        .items;
        Ok(ThreadContextState {
            submissions: std::sync::Arc::new(submissions),
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
            .all(self.store.database())
            .await
            .map_err(store_error)?;
        item_rows.sort_by_key(|row| row.ordinal);
        let items: Vec<ThreadItem> = item_rows
            .into_iter()
            .map(ThreadItem::try_from)
            .collect::<Result<Vec<ThreadItem>, PureError>>()?
            .into_iter()
            .filter(|item| !matches!(item.state(), ThreadItemState::ContextCompaction(_)))
            .collect();
        let active_skills = active_skills_from_items(&items);
        let latest_activation_at = items
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
            has_incomplete_usage: false,
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
    use sea_orm::{ActiveModelTrait, ActiveValue::Set, EntityTrait, IntoActiveModel};

    use crate::studio::entity::{item, thread_context_segment, turn};
    use crate::studio::store::object::put_object;

    use super::super::{StudioAgentRepository, StudioStore, ThreadWriteBehindWriter};
    use super::active_skills_from_items;

    use pl_core::{AgentTurnOutcome, FaultedAgentState, ThreadId, TurnId};
    use pl_protocol::InferenceTokenUsage;
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
            usage: InferenceTokenUsage::default(),
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
    #[test]
    fn restored_active_skills_are_deduped_in_item_order() {
        let items = vec![
            skill_item("item-1", "tool-1", "doc", 1),
            skill_item("item-2", "tool-2", "pdf", 2),
            skill_item("item-3", "tool-3", "doc", 3),
        ];

        assert_eq!(active_skills_from_items(&items), ["doc", "pdf"]);
    }

    #[tokio::test]
    async fn wire_v7_skill_audit_blocks_only_legacy_root_without_rewriting_v13_rows() {
        let store = StudioStore::open_memory().await.expect("memory store");
        let workspace = std::env::temp_dir().join("pure-studio-strict-skill-recovery");
        let project = store.upsert_project(&workspace).await.expect("project");
        let legacy = store
            .create_thread(&project.id, "legacy", crate::ThreadModeId::simple())
            .await
            .expect("legacy thread");
        let healthy = store
            .create_thread(&project.id, "healthy", crate::ThreadModeId::simple())
            .await
            .expect("healthy thread");
        seed_empty_session(&store, &legacy.id).await;
        seed_empty_session(&store, &healthy.id).await;
        seed_completed_turn(&store, &legacy.id, "legacy-turn").await;
        seed_completed_turn(&store, &healthy.id, "healthy-turn").await;

        let legacy_state = serde_json::json!({
            "kind": "skill",
            "data": {
                "activation": {
                    "name": "pdf",
                    "source": "system",
                    "path": "/skills/pdf",
                    "turnId": "legacy-turn",
                    "toolCallId": "tool-legacy",
                    "activatedAt": 7
                }
            }
        })
        .to_string();
        seed_skill_row(
            &store,
            &legacy.id,
            "legacy-turn",
            "legacy-skill",
            &legacy_state,
        )
        .await;
        let healthy_state =
            serde_json::to_string(skill_item("healthy-skill", "tool-healthy", "pdf", 8).state())
                .expect("current Skill JSON");
        seed_skill_row(
            &store,
            &healthy.id,
            "healthy-turn",
            "healthy-skill",
            &healthy_state,
        )
        .await;

        let writer = ThreadWriteBehindWriter::new(store.clone());
        let product_events = crate::studio::ProductEventBus::new(store.clone(), writer.clone());
        let model_performance = crate::studio::runtime::ModelPerformanceOwner::new(
            store.clone(),
            writer.clone(),
            product_events,
        );
        let repository = StudioAgentRepository::with_writer_and_performance(
            store.clone(),
            writer.clone(),
            model_performance,
        );
        assert!(matches!(
            repository.audit_thread_recovery_payloads(&legacy.id).await,
            Err(SessionSnapshotAuditError::Corrupt(_))
        ));
        assert!(
            repository
                .audit_thread_recovery_payloads(&healthy.id)
                .await
                .is_ok(),
            "current v7 Skill remains recoverable"
        );

        let persisted = item::Entity::find_by_id("legacy-skill")
            .one(store.database())
            .await
            .expect("read legacy row")
            .expect("legacy row exists");
        assert_eq!(persisted.state_json, legacy_state);
        writer.shutdown().await.expect("shutdown writer");
    }

    #[tokio::test]
    async fn activation_restores_all_items_and_active_skills() {
        let store = StudioStore::open_memory().await.expect("memory store");
        let workspace = std::env::temp_dir().join("pure-studio-hot-timeline-window");
        let project = store.upsert_project(&workspace).await.expect("project");
        let thread = store
            .create_thread(&project.id, "hot window", crate::ThreadModeId::simple())
            .await
            .expect("thread");
        seed_empty_session(&store, &thread.id).await;
        seed_completed_turn(&store, &thread.id, "old-turn").await;
        seed_completed_turn_at(&store, &thread.id, "new-turn", 1).await;

        for ordinal in 1..=5 {
            seed_text_row(&store, &thread.id, "old-turn", ordinal, "old").await;
        }
        let old_skill = skill_item("old-skill", "tool-old", "pdf", 6);
        item::ActiveModel {
            id: Set(old_skill.id.clone()),
            thread_id: Set(thread.id.clone()),
            turn_id: Set("old-turn".to_string()),
            ordinal: Set(6),
            revision: Set(0),
            state_json: Set(serde_json::to_string(old_skill.state()).unwrap()),
            created_at: Set(6),
            updated_at: Set(6),
            ..Default::default()
        }
        .insert(store.database())
        .await
        .expect("old Skill item");
        for ordinal in 7..=406 {
            seed_text_row(&store, &thread.id, "new-turn", ordinal, "new").await;
        }

        let writer = ThreadWriteBehindWriter::new(store.clone());
        let product_events = crate::studio::ProductEventBus::new(store.clone(), writer.clone());
        let model_performance = crate::studio::runtime::ModelPerformanceOwner::new(
            store.clone(),
            writer.clone(),
            product_events,
        );
        let repository = StudioAgentRepository::with_writer_and_performance(
            store.clone(),
            writer.clone(),
            model_performance,
        );
        let model = crate::studio::entity::thread::Entity::find_by_id(thread.id.clone())
            .one(store.database())
            .await
            .unwrap()
            .unwrap();
        let restored = repository
            .restore_thread_snapshot(model, &pl_core::ThreadContextState::empty())
            .await
            .expect("restore hot snapshot")
            .snapshot;

        assert_eq!(restored.items.len(), 406);
        assert_eq!(restored.items.first().unwrap().turn_id, "old-turn");
        assert_eq!(
            restored
                .runtime
                .expect("runtime from active Skill")
                .active_skills,
            ["pdf"]
        );
        writer.shutdown().await.expect("shutdown writer");
    }

    #[tokio::test]
    async fn startup_pins_budget_paused_agent_without_pending_input() {
        let store = StudioStore::open_memory().await.expect("memory store");
        let workspace = std::env::temp_dir().join("pure-studio-budget-paused-pin");
        let project = store.upsert_project(&workspace).await.expect("project");
        let thread = store
            .create_thread(&project.id, "paused", crate::ThreadModeId::simple())
            .await
            .expect("thread");
        let mut active = crate::studio::entity::thread::Entity::find_by_id(thread.id.clone())
            .one(store.database())
            .await
            .unwrap()
            .unwrap()
            .into_active_model();
        active.state_json = Set(serde_json::to_string(&AgentState::budget_paused(
            pl_core::AgentBudgetPause::new(
                TurnId::new("turn-budget").unwrap(),
                pl_protocol::BudgetLimitSnapshot {
                    kind: pl_protocol::BudgetLimitKind::WallClock,
                    usage: pl_protocol::BudgetUsage::default(),
                },
                10,
            ),
        ))
        .unwrap());
        active.runtime_revision = Set(Some(1));
        active.update(store.database()).await.unwrap();

        let writer = ThreadWriteBehindWriter::new(store.clone());
        let product_events = crate::studio::ProductEventBus::new(store.clone(), writer.clone());
        let model_performance = crate::studio::runtime::ModelPerformanceOwner::new(
            store.clone(),
            writer.clone(),
            product_events,
        );
        let repository = StudioAgentRepository::with_writer_and_performance(
            store,
            writer.clone(),
            model_performance,
        );

        assert_eq!(
            repository.pinned_thread_ids().await.unwrap(),
            BTreeSet::from([thread.id])
        );
        writer.shutdown().await.expect("shutdown writer");
    }

    async fn seed_empty_session(store: &StudioStore, thread_id: &str) {
        let state = pl_protocol::AgentWorkingState::default();
        put_object(store.database(), thread_id, &state, 1)
            .await
            .expect("seed working state");
        assert!(
            thread_context_segment::Entity::find()
                .all(store.database())
                .await
                .expect("read transcript")
                .is_empty()
        );
    }

    async fn seed_completed_turn(store: &StudioStore, thread_id: &str, turn_id: &str) {
        seed_completed_turn_at(store, thread_id, turn_id, 0).await;
    }

    async fn seed_completed_turn_at(
        store: &StudioStore,
        thread_id: &str,
        turn_id: &str,
        ordinal: i64,
    ) {
        let state = pl_protocol::TurnState::Completed(pl_protocol::CompletedTurnState::new(
            Some(1),
            2,
            pl_protocol::TurnCompletion::Normal,
        ));
        turn::ActiveModel {
            id: Set(turn_id.to_string()),
            thread_id: Set(thread_id.to_string()),
            ordinal: Set(ordinal),
            revision: Set(1),
            state_json: Set(serde_json::to_string(&state).expect("turn state JSON")),
            model_json: Set(None),
            usage_json: Set(
                serde_json::to_string(&pl_protocol::InferenceTokenUsage::default()).unwrap(),
            ),
            metadata_json: Set(None),
            updated_at: Set(2),
            ..Default::default()
        }
        .insert(store.database())
        .await
        .expect("seed turn");
    }

    async fn seed_skill_row(
        store: &StudioStore,
        thread_id: &str,
        turn_id: &str,
        item_id: &str,
        state_json: &str,
    ) {
        item::ActiveModel {
            id: Set(item_id.to_string()),
            thread_id: Set(thread_id.to_string()),
            turn_id: Set(turn_id.to_string()),
            ordinal: Set(0),
            revision: Set(0),
            state_json: Set(state_json.to_string()),
            created_at: Set(1),
            updated_at: Set(2),
            ..Default::default()
        }
        .insert(store.database())
        .await
        .expect("seed Skill item");
    }

    async fn seed_text_row(
        store: &StudioStore,
        thread_id: &str,
        turn_id: &str,
        ordinal: i64,
        text: &str,
    ) {
        let state = pl_protocol::ThreadItemState::Text(pl_protocol::ThreadTextItem::new(
            pl_protocol::ThreadTextChannel::Final,
            format!("{text}-{ordinal}"),
            Vec::new(),
            pl_protocol::ThreadContentLifecycle::completed(ordinal),
        ));
        item::ActiveModel {
            id: Set(format!("item-{turn_id}-{ordinal}")),
            thread_id: Set(thread_id.to_string()),
            turn_id: Set(turn_id.to_string()),
            ordinal: Set(ordinal),
            revision: Set(0),
            state_json: Set(serde_json::to_string(&state).unwrap()),
            created_at: Set(ordinal),
            updated_at: Set(ordinal),
            ..Default::default()
        }
        .insert(store.database())
        .await
        .expect("seed text item");
    }

    fn skill_item(
        item_id: &str,
        tool_call_id: &str,
        name: &str,
        activated_at: i64,
    ) -> pl_protocol::ThreadItem {
        pl_protocol::ThreadItem::new(
            item_id.to_string(),
            "thread-1".to_string(),
            "turn-1".to_string(),
            activated_at as u64,
            0,
            activated_at,
            activated_at,
            pl_protocol::ThreadItemState::Skill(pl_protocol::ThreadSkillItem::new(
                pl_protocol::SkillActivation {
                    name: name.to_string(),
                    source: "system".to_string(),
                    provider_id: "local-filesystem".to_string(),
                    resource_base: pl_protocol::SkillActivationResourceBase::Directory {
                        path: format!("/skills/{name}"),
                    },
                    turn_id: "turn-1".to_string(),
                    cause: pl_protocol::SkillActivationCause::Tool {
                        tool_call_id: tool_call_id.to_string(),
                    },
                    activated_at,
                },
            )),
        )
    }
}
