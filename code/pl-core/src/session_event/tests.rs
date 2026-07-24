use pl_protocol::{
    SessionEventEnvelope, SessionEventKind, SessionEventPosition, SessionMessage,
    SessionMessageRole, SessionMessageStatus, SessionPart, SessionPartContent, SessionPartDelta,
    SessionPartDeltaField, SessionPartStatus, SessionRuntimeSnapshot, SessionRuntimeUsage,
    SessionStreamFrame, SessionSubscriptionRequest, SessionTextChannel, SkillActivation,
};
use pretty_assertions::assert_eq;

use super::{SessionEventError, SessionEventHub, SessionEventOptions};

#[tokio::test]
async fn subscriptions_are_isolated_by_session() {
    let hub = SessionEventHub::default();
    let mut a = hub
        .subscribe(SessionSubscriptionRequest::new("a"))
        .expect("subscribe a");
    let mut b = hub
        .subscribe(SessionSubscriptionRequest::new("b"))
        .expect("subscribe b");
    assert!(matches!(
        a.recv().await,
        Some(SessionStreamFrame::Snapshot { .. })
    ));
    assert!(matches!(
        b.recv().await,
        Some(SessionStreamFrame::Snapshot { .. })
    ));

    hub.publish_durable(message_event("a", 1, "a-message"))
        .expect("publish");
    let Some(SessionStreamFrame::Event { event }) = a.recv().await else {
        panic!("expected event");
    };
    assert_eq!(event.session_id, "a");
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(10), b.recv())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn cursor_replays_only_newer_durable_events() {
    let hub = SessionEventHub::default();
    hub.publish_durable(message_event("a", 1, "one"))
        .expect("one");
    hub.publish_durable(message_event("a", 2, "two"))
        .expect("two");
    let mut subscription = hub
        .subscribe(SessionSubscriptionRequest::new("a").after(1))
        .expect("subscribe");
    let Some(SessionStreamFrame::Event { event }) = subscription.recv().await else {
        panic!("expected replay");
    };
    assert_eq!(event.position.durable_sequence(), Some(2));
}

#[tokio::test]
async fn expired_cursor_receives_snapshot() {
    let hub = SessionEventHub::new(SessionEventOptions {
        retained_durable_events: 1,
        ..SessionEventOptions::default()
    });
    hub.publish_durable(message_event("a", 1, "one"))
        .expect("one");
    hub.publish_durable(message_event("a", 2, "two"))
        .expect("two");
    let mut subscription = hub
        .subscribe(SessionSubscriptionRequest::new("a").after(0))
        .expect("subscribe");
    let Some(SessionStreamFrame::Snapshot { snapshot }) = subscription.recv().await else {
        panic!("expected snapshot");
    };
    assert_eq!(snapshot.through_sequence, 2);
    assert_eq!(snapshot.messages.len(), 2);
}

#[tokio::test]
async fn multiple_subscribers_receive_the_same_committed_event() {
    let hub = SessionEventHub::default();
    let mut first = hub
        .subscribe(SessionSubscriptionRequest::new("a"))
        .expect("first");
    let mut second = hub
        .subscribe(SessionSubscriptionRequest::new("a"))
        .expect("second");
    first.recv().await.expect("first snapshot");
    second.recv().await.expect("second snapshot");

    hub.publish_durable(message_event("a", 1, "message"))
        .expect("publish");

    assert!(matches!(
        first.recv().await,
        Some(SessionStreamFrame::Event { .. })
    ));
    assert!(matches!(
        second.recv().await,
        Some(SessionStreamFrame::Event { .. })
    ));
}

#[tokio::test]
async fn subscribe_barrier_delivers_snapshot_before_a_concurrent_live_event() {
    let hub = SessionEventHub::default();
    let mut subscription = hub
        .subscribe(SessionSubscriptionRequest::new("a"))
        .expect("subscribe");
    hub.publish_durable(message_event("a", 1, "message"))
        .expect("publish");

    let Some(SessionStreamFrame::Snapshot { snapshot }) = subscription.recv().await else {
        panic!("expected bootstrap snapshot");
    };
    assert_eq!(snapshot.through_sequence, 0);
    let Some(SessionStreamFrame::Event { event }) = subscription.recv().await else {
        panic!("expected live event");
    };
    assert_eq!(event.position.durable_sequence(), Some(1));
}

#[tokio::test]
async fn replay_larger_than_limit_falls_back_to_a_snapshot() {
    let hub = SessionEventHub::new(SessionEventOptions {
        replay_limit: 1,
        ..SessionEventOptions::default()
    });
    hub.publish_durable(message_event("a", 1, "one"))
        .expect("one");
    hub.publish_durable(message_event("a", 2, "two"))
        .expect("two");
    let mut subscription = hub
        .subscribe(SessionSubscriptionRequest::new("a").after(0))
        .expect("subscribe");

    let Some(SessionStreamFrame::Snapshot { snapshot }) = subscription.recv().await else {
        panic!("expected snapshot");
    };
    assert_eq!(snapshot.through_sequence, 2);
}

#[tokio::test]
async fn lag_emits_one_resync_frame_and_terminates_the_subscription() {
    let hub = SessionEventHub::new(SessionEventOptions {
        channel_capacity: 1,
        ..SessionEventOptions::default()
    });
    let mut subscription = hub
        .subscribe(SessionSubscriptionRequest::new("a"))
        .expect("subscribe");
    subscription.recv().await.expect("snapshot");
    for sequence in 1..=3 {
        hub.publish_durable(message_event("a", sequence, &format!("message-{sequence}")))
            .expect("publish");
    }

    assert!(matches!(
        subscription.recv().await,
        Some(SessionStreamFrame::ResyncRequired { .. })
    ));
    assert_eq!(subscription.recv().await, None);
}

#[tokio::test]
async fn invalid_batch_is_neither_applied_nor_partially_broadcast() {
    let hub = SessionEventHub::default();
    let handle = hub.handle();
    let mut subscription = hub
        .subscribe(SessionSubscriptionRequest::new("a"))
        .expect("subscribe");
    subscription.recv().await.expect("snapshot");

    let error = handle
        .publish_batch(vec![
            message_event("a", 1, "one"),
            message_event("a", 3, "three"),
        ])
        .expect_err("sequence gap");

    assert_eq!(
        error,
        SessionEventError::SequenceGap {
            expected: 2,
            actual: 3,
        }
    );
    assert_eq!(hub.snapshot("a").expect("snapshot").through_sequence, 0);
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(10), subscription.recv())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn transient_revision_gap_does_not_modify_the_live_overlay() {
    let hub = SessionEventHub::default();
    hub.publish_durable(part_event("a", 1, "part"))
        .expect("part");
    let before = hub.snapshot("a").expect("before");

    let error = hub
        .publish_transient(delta_event("a", "part", 2, "skipped"))
        .expect_err("revision gap");

    assert_eq!(
        error,
        SessionEventError::RevisionGap {
            part_id: "part".to_string(),
            expected: 1,
            actual: 2,
        }
    );
    assert_eq!(hub.snapshot("a").expect("after"), before);
}

#[test]
fn skill_fact_keeps_runtime_metadata_in_sync() {
    let hub = SessionEventHub::default();
    hub.publish_durable(SessionEventEnvelope {
        event_id: "a:1".to_string(),
        session_id: "a".to_string(),
        source_agent_id: Some("root".to_string()),
        turn_id: Some("turn".to_string()),
        emitted_at: 1,
        position: SessionEventPosition::Durable { sequence: 1 },
        kind: SessionEventKind::RuntimeChanged {
            runtime: Box::new(SessionRuntimeSnapshot {
                session_id: "a".to_string(),
                usage: SessionRuntimeUsage {
                    model: "model".to_string(),
                    context_window: Some(128_000),
                    latest_context_tokens: 0,
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    cached_prompt_tokens: 0,
                    total_tokens: 0,
                    cache_hit_rate: None,
                    estimated_costs: Vec::new(),
                    has_unpriced_usage: false,
                    updated_at: 1,
                },
                active_skills: Vec::new(),
                active_mcp_servers: vec!["search".to_string()],
                active_lsp_servers: vec!["rust-analyzer".to_string()],
                agent_count: 0,
                mcp_health: None,
                updated_at: 1,
            }),
        },
    })
    .unwrap();
    hub.publish_durable(SessionEventEnvelope {
        event_id: "a:2".to_string(),
        session_id: "a".to_string(),
        source_agent_id: Some("root".to_string()),
        turn_id: Some("turn".to_string()),
        emitted_at: 2,
        position: SessionEventPosition::Durable { sequence: 2 },
        kind: SessionEventKind::SkillActivated {
            activation: SkillActivation {
                name: "review".to_string(),
                source: "project".to_string(),
                path: "/project/repo/skills/review/SKILL.md".to_string(),
                turn_id: "turn".to_string(),
                tool_call_id: "skill-call".to_string(),
                activated_at: 2,
            },
        },
    })
    .unwrap();
    let snapshot = hub.snapshot("a").unwrap();
    let runtime = snapshot.runtime.unwrap();
    assert_eq!(runtime.active_skills, vec!["review".to_string()]);
    assert_eq!(runtime.active_mcp_servers, vec!["search".to_string()]);
    assert_eq!(runtime.agent_count, 0);
}

#[test]
fn session_rejects_events_from_a_different_owner_agent() {
    let hub = SessionEventHub::default();
    hub.publish_durable(message_event("a", 1, "root-message"))
        .expect("establish owner");
    let mut child_event = message_event("a", 2, "child-message");
    child_event.source_agent_id = Some("child".to_string());

    let error = hub
        .publish_durable(child_event)
        .expect_err("cross-agent event must be rejected");

    assert!(matches!(error, SessionEventError::ProjectionInvariant(_)));
    let snapshot = hub.snapshot("a").expect("snapshot");
    assert_eq!(snapshot.messages.len(), 1);
    assert_eq!(
        snapshot.owner.as_ref().map(|owner| owner.agent_id.as_str()),
        Some("agent")
    );
}

fn message_event(session_id: &str, sequence: u64, message_id: &str) -> SessionEventEnvelope {
    SessionEventEnvelope {
        event_id: format!("{session_id}:{sequence}"),
        session_id: session_id.to_string(),
        source_agent_id: Some("agent".to_string()),
        turn_id: Some("turn".to_string()),
        emitted_at: sequence as i64,
        position: SessionEventPosition::Durable { sequence },
        kind: SessionEventKind::MessageChanged {
            message: Box::new(SessionMessage {
                message_id: message_id.to_string(),
                session_id: session_id.to_string(),
                turn_id: "turn".to_string(),
                role: SessionMessageRole::Assistant,
                status: SessionMessageStatus::Streaming,
                created_at: 1,
                updated_at: 1,
                completed_at: None,
                error: None,
                metadata: serde_json::json!({}),
            }),
        },
    }
}

fn part_event(session_id: &str, sequence: u64, part_id: &str) -> SessionEventEnvelope {
    SessionEventEnvelope {
        event_id: format!("{session_id}:{sequence}"),
        session_id: session_id.to_string(),
        source_agent_id: Some("agent".to_string()),
        turn_id: Some("turn".to_string()),
        emitted_at: sequence as i64,
        position: SessionEventPosition::Durable { sequence },
        kind: SessionEventKind::PartChanged {
            part: Box::new(SessionPart {
                part_id: part_id.to_string(),
                message_id: "message".to_string(),
                session_id: session_id.to_string(),
                turn_id: "turn".to_string(),
                order: 0,
                revision: 0,
                status: SessionPartStatus::Streaming,
                created_at: 1,
                updated_at: 1,
                completed_at: None,
                error: None,
                content: SessionPartContent::Text {
                    channel: SessionTextChannel::Commentary,
                    text: String::new(),
                    attachments: Vec::new(),
                },
                usage: None,
                synthetic: false,
                ignored: false,
            }),
        },
    }
}

fn delta_event(
    session_id: &str,
    part_id: &str,
    revision: u64,
    delta: &str,
) -> SessionEventEnvelope {
    SessionEventEnvelope {
        event_id: format!("{session_id}:delta:{revision}"),
        session_id: session_id.to_string(),
        source_agent_id: Some("agent".to_string()),
        turn_id: Some("turn".to_string()),
        emitted_at: revision as i64,
        position: SessionEventPosition::Transient { revision },
        kind: SessionEventKind::PartDelta {
            delta: SessionPartDelta {
                part_id: part_id.to_string(),
                revision,
                field: SessionPartDeltaField::Text,
                delta: delta.to_string(),
                chunk_index: None,
            },
        },
    }
}
