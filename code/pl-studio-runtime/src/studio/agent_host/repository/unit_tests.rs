//! repository 输入元数据与预算恢复投影测试。

use pl_core::{AgentTurnOutcome, TurnId};

use crate::studio::entity::turn;

use super::input_metadata::{deserialize_input_metadata, serialize_input_metadata};

use super::*;
use pl_core::MailboxPresentation;
use pl_protocol::TurnOutcome;

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
