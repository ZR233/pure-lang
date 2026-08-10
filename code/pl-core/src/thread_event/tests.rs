use pl_protocol::{
    AgentMessageChannel, ThreadItem, ThreadItemContent, ThreadItemDelta, ThreadItemDeltaField,
    ThreadItemStatus, ThreadNotification, ThreadNotificationEnvelope, ThreadSubscriptionRequest,
    ThreadSubscriptionUpdate, Turn, TurnPhase, TurnState,
};
use tokio::time::{Duration, timeout};

use super::*;

#[tokio::test]
async fn subscription_registers_before_returning_snapshot() {
    let bus = ThreadEventBus::default();
    bus.replace_snapshot(ThreadSnapshot::empty("thread-1"))
        .unwrap();
    let mut subscription = bus
        .subscribe(ThreadSubscriptionRequest {
            thread_id: "thread-1".to_string(),
        })
        .unwrap();
    bus.publish(notification(1, item_started("hello")))
        .await
        .unwrap();

    assert!(matches!(
        subscription.recv().await,
        Some(ThreadSubscriptionUpdate::Snapshot { snapshot }) if snapshot.revision == 0
    ));
    assert!(matches!(
        subscription.recv().await,
        Some(ThreadSubscriptionUpdate::Notification { notification })
            if notification.revision == 1
    ));
}

#[tokio::test]
async fn product_metadata_rebinds_only_the_subscription_bootstrap() {
    let bus = ThreadEventBus::default();
    bus.replace_snapshot(ThreadSnapshot::empty("thread-1"))
        .unwrap();
    let mut subscription = bus
        .subscribe(ThreadSubscriptionRequest {
            thread_id: "thread-1".to_string(),
        })
        .unwrap();
    let mut thread = pl_protocol::Thread::placeholder("thread-1");
    thread.mode = pl_protocol::ThreadMode::Task;
    thread.role = "planner".to_string();

    subscription.replace_bootstrap_thread(thread).unwrap();

    assert!(matches!(
        subscription.recv().await,
        Some(ThreadSubscriptionUpdate::Snapshot { snapshot })
            if snapshot.thread.mode == pl_protocol::ThreadMode::Task
                && snapshot.thread.role == "planner"
    ));
    assert_eq!(
        bus.snapshot("thread-1").unwrap().thread.mode,
        pl_protocol::ThreadMode::Simple
    );
}

#[test]
fn unknown_thread_cannot_be_read_or_subscribed() {
    let bus = ThreadEventBus::default();

    assert!(matches!(
        bus.snapshot("missing"),
        Err(ThreadEventError::ThreadNotFound(thread_id)) if thread_id == "missing"
    ));
    assert!(matches!(
        bus.subscribe(ThreadSubscriptionRequest {
            thread_id: "missing".to_string(),
        }),
        Err(ThreadEventError::ThreadNotFound(thread_id)) if thread_id == "missing"
    ));
}

#[tokio::test]
async fn canonical_snapshot_rejects_revision_gap() {
    let bus = ThreadEventBus::default();
    let error = bus
        .publish(notification(2, item_started("late")))
        .await
        .unwrap_err();
    assert!(matches!(error, ThreadEventError::RevisionGap { .. }));
}

#[tokio::test]
async fn turn_and_item_lifecycle_updates_the_authoritative_snapshot() {
    let bus = ThreadEventBus::default();
    bus.replace_snapshot(ThreadSnapshot::empty("thread-1"))
        .unwrap();
    bus.publish(notification(
        1,
        ThreadNotification::TurnStarted {
            turn: active_turn(TurnPhase::Responding),
        },
    ))
    .await
    .unwrap();
    bus.publish(notification(2, item_started("hello")))
        .await
        .unwrap();
    bus.publish(notification(
        3,
        ThreadNotification::ItemDelta {
            delta: ThreadItemDelta {
                item_id: "item-1".to_string(),
                revision: 1,
                field: ThreadItemDeltaField::Text,
                delta: " world".to_string(),
                chunk_index: None,
            },
        },
    ))
    .await
    .unwrap();
    let mut completed_item = match item_started("hello world") {
        ThreadNotification::ItemStarted { item } => item,
        _ => unreachable!("fixture always creates ItemStarted"),
    };
    completed_item.revision = 2;
    completed_item.status = ThreadItemStatus::Completed;
    completed_item.completed_at = Some(5);
    bus.publish(notification(
        4,
        ThreadNotification::ItemCompleted {
            item: completed_item,
        },
    ))
    .await
    .unwrap();
    bus.publish(notification(
        5,
        ThreadNotification::TurnCompleted {
            turn: Turn {
                state: TurnState::Completed,
                completed_at: Some(5),
                ..active_turn(TurnPhase::Persisting)
            },
        },
    ))
    .await
    .unwrap();

    let snapshot = bus.snapshot("thread-1").unwrap();
    assert_eq!(snapshot.revision, 5);
    assert!(snapshot.active_turn.is_none());
    assert_eq!(snapshot.items.len(), 1);
    assert_eq!(snapshot.items[0].ordinal, 1);
    assert_eq!(snapshot.items[0].revision, 2);
    assert_eq!(snapshot.items[0].status, ThreadItemStatus::Completed);
    assert!(matches!(
        &snapshot.items[0].content,
        ThreadItemContent::AgentMessage { text, .. } if text == "hello world"
    ));
}

#[tokio::test]
async fn lossless_notification_waits_for_subscriber_capacity() {
    let bus = ThreadEventBus::new(ThreadEventOptions {
        channel_capacity: 1,
    });
    bus.replace_snapshot(ThreadSnapshot::empty("thread-1"))
        .unwrap();
    let mut subscription = bus
        .subscribe(ThreadSubscriptionRequest {
            thread_id: "thread-1".to_string(),
        })
        .unwrap();
    assert!(matches!(
        subscription.recv().await,
        Some(ThreadSubscriptionUpdate::Snapshot { .. })
    ));
    bus.publish(notification(1, item_started("hello")))
        .await
        .unwrap();

    let publisher_bus = bus.clone();
    let mut publisher = tokio::spawn(async move {
        publisher_bus
            .publish(notification(
                2,
                ThreadNotification::ItemDelta {
                    delta: ThreadItemDelta {
                        item_id: "item-1".to_string(),
                        revision: 1,
                        field: ThreadItemDeltaField::Text,
                        delta: " world".to_string(),
                        chunk_index: None,
                    },
                },
            ))
            .await
    });
    assert!(
        timeout(Duration::from_millis(20), &mut publisher)
            .await
            .is_err()
    );
    assert!(matches!(
        subscription.recv().await,
        Some(ThreadSubscriptionUpdate::Notification { notification })
            if notification.revision == 1
    ));
    publisher.await.unwrap().unwrap();
    assert!(matches!(
        subscription.recv().await,
        Some(ThreadSubscriptionUpdate::Notification { notification })
            if notification.revision == 2
    ));
}

#[tokio::test]
async fn best_effort_drop_reports_lag_before_the_next_lossless_notification() {
    let bus = ThreadEventBus::new(ThreadEventOptions {
        channel_capacity: 1,
    });
    bus.replace_snapshot(ThreadSnapshot::empty("thread-1"))
        .unwrap();
    let mut subscription = bus
        .subscribe(ThreadSubscriptionRequest {
            thread_id: "thread-1".to_string(),
        })
        .unwrap();
    assert!(matches!(
        subscription.recv().await,
        Some(ThreadSubscriptionUpdate::Snapshot { .. })
    ));
    bus.publish(notification(
        1,
        ThreadNotification::TurnUpdated {
            turn: active_turn(TurnPhase::Thinking),
        },
    ))
    .await
    .unwrap();
    bus.publish(notification(
        2,
        ThreadNotification::TurnUpdated {
            turn: active_turn(TurnPhase::Responding),
        },
    ))
    .await
    .unwrap();

    let publisher_bus = bus.clone();
    let publisher = tokio::spawn(async move {
        publisher_bus
            .publish(notification(
                3,
                ThreadNotification::TurnCompleted {
                    turn: Turn {
                        state: TurnState::Completed,
                        completed_at: Some(3),
                        ..active_turn(TurnPhase::Persisting)
                    },
                },
            ))
            .await
    });
    assert!(matches!(
        subscription.recv().await,
        Some(ThreadSubscriptionUpdate::Notification { notification })
            if notification.revision == 1
    ));
    assert!(matches!(
        subscription.recv().await,
        Some(ThreadSubscriptionUpdate::Notification { notification })
            if matches!(notification.notification, ThreadNotification::Lagged { dropped: 1 })
    ));
    assert!(matches!(
        subscription.recv().await,
        Some(ThreadSubscriptionUpdate::Notification { notification })
            if matches!(notification.notification, ThreadNotification::TurnCompleted { .. })
    ));
    publisher.await.unwrap().unwrap();
}

fn notification(revision: u64, notification: ThreadNotification) -> ThreadNotificationEnvelope {
    ThreadNotificationEnvelope {
        thread_id: "thread-1".to_string(),
        revision,
        emitted_at: 1,
        notification,
    }
}

fn item_started(text: &str) -> ThreadNotification {
    ThreadNotification::ItemStarted {
        item: Box::new(ThreadItem {
            id: "item-1".to_string(),
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            ordinal: 1,
            revision: 0,
            status: ThreadItemStatus::Started,
            created_at: 1,
            updated_at: 1,
            completed_at: None,
            error: None,
            content: ThreadItemContent::AgentMessage {
                channel: AgentMessageChannel::Commentary,
                text: text.to_string(),
            },
            usage: None,
        }),
    }
}

fn active_turn(phase: TurnPhase) -> Turn {
    Turn {
        id: "turn-1".to_string(),
        thread_id: "thread-1".to_string(),
        state: TurnState::InProgress { phase },
        started_at: Some(1),
        updated_at: 1,
        completed_at: None,
        failure: None,
    }
}
