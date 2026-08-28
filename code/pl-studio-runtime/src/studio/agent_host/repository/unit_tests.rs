//! repository 输入元数据、预算恢复投影与活动 Turn fallback 测试。

use std::collections::VecDeque;

use pl_core::{
    AgentIdentity, AgentRoleId, AgentSnapshot, AgentState, AgentTurnOutcome, DurableCommitFacts,
    MailboxPresentation, PersistenceClass, RunningAgentState, ThreadActorState, ThreadContextState,
    ThreadId, ThreadMutation, TurnId,
};
use pl_protocol::{
    RunningTurnState, ThreadNotification, ThreadNotificationEnvelope, Turn, TurnOutcome, TurnPhase,
    TurnState,
};
use sea_orm::{ActiveModelTrait, ActiveValue::Set, EntityTrait, TransactionTrait};

use crate::studio::entity::{
    item, thread_context_segment, thread_input, thread_session_state, turn,
};

use super::input_metadata::{deserialize_input_metadata, serialize_input_metadata};

use super::*;

use super::restore::active_skills_from_items;
use crate::StudioMode;

#[test]
fn thread_input_restores_typed_attachment_manifest() {
    let attachment = pl_protocol::ThreadAttachment {
        id: "attachment-1".to_string(),
        modality: pl_protocol::AttachmentModality::Image,
        media_type: "image/png".to_string(),
        filename: Some("marker.png".to_string()),
        width: Some(1200),
        height: Some(800),
        byte_size: 80_000,
    };
    let restored = pl_core::DurableMailboxEnvelope::try_from(thread_input::Model {
        id: "mail-1".to_string(),
        thread_id: "thread-1".to_string(),
        mail_id: "mail-1".to_string(),
        turn_id: "turn-1".to_string(),
        content: "inspect".to_string(),
        attachments_json: serde_json::to_string(std::slice::from_ref(&attachment)).unwrap(),
        metadata_json: "null".to_string(),
        presentation: "user".to_string(),
        state_json: serde_json::to_string(&pl_core::MailboxDeliveryState::default()).unwrap(),
        state_kind: "pending".to_string(),
        queue_ordinal: 0,
        queued_at: 7,
    })
    .unwrap();

    assert_eq!(restored.payload.attachments, [attachment]);
}

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
    let state = pl_protocol::TurnState::BudgetLimited(pl_protocol::BudgetLimitedTurnState::new(
        Some(1),
        2,
        limit,
        pl_protocol::TurnRolloverOutcome::Succeeded,
    ));
    let outcome = AgentTurnOutcome::try_from(turn::Model {
        id: "turn-budget".to_string(),
        thread_id: "thread-budget".to_string(),
        ordinal: 0,
        revision: 1,
        state_json: serde_json::to_string(&state).unwrap(),
        state_kind: "budgetLimited".to_string(),
        model_json: None,
        usage_json: serde_json::to_string(&pl_model::TokenUsage::default()).unwrap(),
        metadata_json: None,
        updated_at: 2,
    })
    .unwrap();

    assert_eq!(
        outcome.outcome,
        TurnOutcome::budget_limited(limit, pl_protocol::TurnRolloverOutcome::Succeeded)
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
            attachments: Vec::new(),
            presentation: MailboxPresentation::Hidden,
            metadata: serde_json::json!({"kind": "taskWake"}),
        },
        queue_coalescing_key: Some("task-run:wakes".to_string()),
        budget_action: pl_core::MailboxBudgetAction::Preserve,
        delivery_state: MailboxDeliveryState::default(),
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
            attachments: Vec::new(),
            presentation: MailboxPresentation::Hidden,
            metadata: serde_json::json!({"kind": "plannerMessage"}),
        },
        queue_coalescing_key: None,
        budget_action: pl_core::MailboxBudgetAction::Refresh,
        delivery_state: MailboxDeliveryState::default(),
        queued_at: 1,
    };

    let stored = serialize_input_metadata(&input).unwrap();
    let (metadata, key, budget_action) = deserialize_input_metadata(&stored).unwrap();

    assert_eq!(metadata, input.payload.metadata);
    assert_eq!(key, None);
    assert_eq!(budget_action, pl_core::MailboxBudgetAction::Refresh);
}

#[test]
fn payload_only_input_metadata_remains_unwrapped() {
    let stored = r#"{"kind":"taskWake"}"#;
    let (metadata, key, budget_action) = deserialize_input_metadata(stored).unwrap();

    assert_eq!(metadata, serde_json::json!({"kind": "taskWake"}));
    assert_eq!(key, None);
    assert_eq!(budget_action, pl_core::MailboxBudgetAction::Preserve);
}

#[tokio::test]
async fn active_turn_fallback_preserves_canonical_phase() {
    let store = StudioStore::open_memory().await.expect("memory store");
    let project = store
        .upsert_project(std::env::temp_dir().join("active-turn-fallback"))
        .await
        .expect("project");
    let thread = store
        .create_thread(&project.id, "active-turn-fallback", StudioMode::Simple)
        .await
        .expect("thread");
    let thread_id = thread.id;

    // 既有的 canonical Thinking phase 不能被粗粒度 AgentState fallback 覆盖。
    seed_running_turn(&store, &thread_id, "turn-thinking", 0, TurnPhase::Thinking).await;
    {
        let tx = store.database().begin().await.expect("begin");
        let state = running_actor_state(&thread_id, "turn-thinking", 9);
        persist_state_turns(&tx, &state)
            .await
            .expect("write-behind");
        tx.commit().await.expect("commit");
    }
    assert_phase(&store, &thread_id, "turn-thinking", TurnPhase::Thinking).await;

    // 既有的 canonical RunningTool phase 同样被保留。
    seed_running_turn(&store, &thread_id, "turn-tool", 1, TurnPhase::RunningTool).await;
    {
        let tx = store.database().begin().await.expect("begin");
        let state = running_actor_state(&thread_id, "turn-tool", 10);
        persist_state_turns(&tx, &state)
            .await
            .expect("write-behind");
        tx.commit().await.expect("commit");
    }
    assert_phase(&store, &thread_id, "turn-tool", TurnPhase::RunningTool).await;

    // 缺失 Turn 行仍建立粗粒度 fallback。
    {
        let tx = store.database().begin().await.expect("begin");
        let state = running_actor_state(&thread_id, "turn-missing", 11);
        persist_state_turns(&tx, &state)
            .await
            .expect("write-behind");
        tx.commit().await.expect("commit");
    }
    assert_phase(&store, &thread_id, "turn-missing", TurnPhase::Responding).await;

    // 后续 canonical ThreadNotification 把 fallback 覆盖为精确 phase。
    {
        let tx = store.database().begin().await.expect("begin");
        let state = running_actor_state(&thread_id, "turn-missing", 12);
        let commit = turn_update_commit(state, "turn-missing", TurnPhase::Thinking, 12);
        persist_thread_notifications(&tx, &commit)
            .await
            .expect("notification");
        tx.commit().await.expect("commit");
    }
    assert_phase(&store, &thread_id, "turn-missing", TurnPhase::Thinking).await;
}

fn running_actor_state(thread_id: &str, turn_id: &str, updated_at: i64) -> ThreadActorState {
    let thread_id = ThreadId::new(thread_id).expect("thread id");
    let turn_id = TurnId::new(turn_id).expect("turn id");
    ThreadActorState {
        snapshot: AgentSnapshot {
            identity: AgentIdentity {
                id: thread_id.clone(),
                parent_id: None,
                role: AgentRoleId::new("executor").expect("role"),
                depth: 0,
            },
            state: AgentState::Running(RunningAgentState::new(turn_id)),
            pending_inputs: 0,
            progress: None,
            last_turn: None,
            revision: 1,
            event_sequence: 1,
            updated_at,
        },
        session: ThreadContextState::empty(),
        pending_inputs: VecDeque::new(),
        active_input: None,
    }
}

fn turn_update_commit(
    state: ThreadActorState,
    turn_id: &str,
    phase: TurnPhase,
    emitted_at: i64,
) -> ThreadCommit {
    let thread_id = state.snapshot.identity.id.clone();
    let turn_id_ref = TurnId::new(turn_id).expect("turn id");
    ThreadCommit {
        agent_id: thread_id.clone(),
        persistence: PersistenceClass::Standard,
        expected_revision: None,
        next_state: state,
        facts: DurableCommitFacts {
            thread_id: thread_id.clone(),
            turn_id: Some(turn_id_ref),
            through_revision: 0,
            revision: 1,
            notifications: vec![ThreadNotificationEnvelope {
                thread_id: thread_id.to_string(),
                revision: 1,
                emitted_at,
                notification: ThreadNotification::TurnUpdated {
                    turn: Turn {
                        id: turn_id.to_string(),
                        thread_id: thread_id.to_string(),
                        revision: 1,
                        state: TurnState::Running(RunningTurnState::new(emitted_at, phase)),
                        updated_at: emitted_at,
                    },
                },
            }],
            turn_transition: None,
            context: None,
            projection_snapshot: None,
            runtime_events: Vec::new(),
            trace_events: Vec::new(),
            inference: None,
            submission: None,
        },
        mutation: ThreadMutation::ReplaceThread { thread_id },
    }
}

async fn seed_running_turn(
    store: &StudioStore,
    thread_id: &str,
    turn_id: &str,
    ordinal: i64,
    phase: TurnPhase,
) {
    turn::ActiveModel {
        id: Set(turn_id.to_string()),
        thread_id: Set(thread_id.to_string()),
        ordinal: Set(ordinal),
        revision: Set(1),
        state_json: Set(
            serde_json::to_string(&TurnState::Running(RunningTurnState::new(1, phase)))
                .expect("turn state JSON"),
        ),
        model_json: Set(None),
        usage_json: Set(serde_json::to_string(&pl_model::TokenUsage::default()).unwrap()),
        metadata_json: Set(None),
        updated_at: Set(1),
        ..Default::default()
    }
    .insert(store.database())
    .await
    .expect("seed running turn");
}

async fn assert_phase(store: &StudioStore, thread_id: &str, turn_id: &str, expected: TurnPhase) {
    let row = turn::Entity::find_by_id(turn_id)
        .one(store.database())
        .await
        .expect("read turn")
        .expect("turn row exists");
    assert_eq!(row.thread_id, thread_id, "turn {turn_id} belongs to thread");
    let state = serde_json::from_str::<TurnState>(&row.state_json).expect("parse turn state");
    assert_eq!(
        state.phase(),
        Some(expected),
        "turn {turn_id} must keep phase {expected:?}"
    );
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
        .create_thread(&project.id, "legacy", crate::StudioMode::Simple)
        .await
        .expect("legacy thread");
    let healthy = store
        .create_thread(&project.id, "healthy", crate::StudioMode::Simple)
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
    let repository = StudioAgentRepository::with_writer(store.clone(), writer.clone());
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

async fn seed_empty_session(store: &StudioStore, thread_id: &str) {
    let state = pl_protocol::AgentWorkingState::default();
    let state_json = serde_json::to_string(&state).expect("working state JSON");
    thread_session_state::ActiveModel {
        thread_id: Set(thread_id.to_string()),
        revision: Set(0),
        state_hash: Set(pl_core::canonical_content_hash(state_json.as_bytes())),
        state_json: Set(state_json),
        updated_at: Set(1),
    }
    .insert(store.database())
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
    let state = pl_protocol::TurnState::Completed(pl_protocol::CompletedTurnState::new(
        Some(1),
        2,
        pl_protocol::TurnCompletion::Normal,
    ));
    turn::ActiveModel {
        id: Set(turn_id.to_string()),
        thread_id: Set(thread_id.to_string()),
        ordinal: Set(0),
        revision: Set(1),
        state_json: Set(serde_json::to_string(&state).expect("turn state JSON")),
        model_json: Set(None),
        usage_json: Set(serde_json::to_string(&pl_model::TokenUsage::default()).unwrap()),
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
