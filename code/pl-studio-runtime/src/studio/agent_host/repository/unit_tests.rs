//! repository 输入元数据与预算恢复投影测试。

use pl_core::{AgentTurnOutcome, TurnId};
use sea_orm::{ActiveModelTrait, ActiveValue::Set, EntityTrait};

use crate::studio::entity::{item, thread_context_segment, thread_session_state, turn};

use super::input_metadata::{deserialize_input_metadata, serialize_input_metadata};

use super::*;
use pl_core::MailboxPresentation;
use pl_protocol::TurnOutcome;

use super::restore::active_skills_from_items;

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
