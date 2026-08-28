use pl_protocol::{
    CompletedTurnState, RunningTurnState, ThreadContentLifecycle, ThreadContextDisposition,
    ThreadItem, ThreadItemDelta, ThreadItemDeltaState, ThreadItemState, ThreadNotification,
    ThreadNotificationEnvelope, ThreadSubscriptionRequest, ThreadSubscriptionUpdate,
    ThreadTextChannel, ThreadTextItem, ThreadTurnHistory, Turn, TurnCompletion, TurnPhase,
    TurnState,
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
async fn removing_thread_releases_snapshot_and_closes_subscription() {
    let bus = ThreadEventBus::default();
    bus.replace_snapshot(ThreadSnapshot::empty("thread-1"))
        .unwrap();
    let mut subscription = bus
        .subscribe(ThreadSubscriptionRequest {
            thread_id: "thread-1".to_string(),
        })
        .unwrap();

    assert!(bus.remove_thread("thread-1").unwrap());
    assert!(matches!(
        bus.snapshot("thread-1"),
        Err(ThreadEventError::ThreadNotFound(thread_id)) if thread_id == "thread-1"
    ));
    assert!(matches!(
        subscription.recv().await,
        Some(ThreadSubscriptionUpdate::Snapshot { .. })
    ));
    assert!(subscription.recv().await.is_none());
    assert!(!bus.remove_thread("thread-1").unwrap());
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
fn cold_page_materializes_into_hot_window_without_overwriting_hot_items() {
    let bus = ThreadEventBus::default();
    let mut snapshot = ThreadSnapshot::empty("thread-1");
    snapshot.active_turn = Some(active_turn(TurnPhase::Responding));
    let hot_item = commentary_item("hot", 1, ThreadContentLifecycle::completed(2));
    snapshot.items = vec![hot_item.clone()];
    bus.replace_snapshot(snapshot).unwrap();
    let older_turn = Turn::queued("turn-old", "thread-1", 0);
    let histories = vec![
        ThreadTurnHistory {
            turn: active_turn(TurnPhase::Responding),
            items: vec![commentary_item(
                "cold-stale",
                0,
                ThreadContentLifecycle::completed(1),
            )],
            context_disposition: ThreadContextDisposition::Active,
        },
        ThreadTurnHistory {
            turn: older_turn,
            items: vec![user_message_item("cold-only", 0)],
            context_disposition: ThreadContextDisposition::Active,
        },
    ];

    bus.merge_cold_history("thread-1", &histories).unwrap();

    let hot = bus.hot_history("thread-1").unwrap();
    assert_eq!(hot.turns.len(), 2);
    assert_eq!(hot.items.len(), 2);
    assert_eq!(
        hot.items
            .iter()
            .find(|item| item.id == hot_item.id)
            .unwrap(),
        &hot_item
    );
    assert!(hot.items.iter().any(|item| item.id == "cold-only"));
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
                delta: ThreadItemDeltaState::Text {
                    delta: " world".to_string(),
                },
            },
        },
    ))
    .await
    .unwrap();
    let completed_item = Box::new(commentary_item(
        "hello world",
        2,
        ThreadContentLifecycle::completed(5),
    ));
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
                state: TurnState::Completed(CompletedTurnState::new(
                    Some(1),
                    5,
                    TurnCompletion::Normal,
                )),
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
    assert!(matches!(
        snapshot.items[0].state(),
        ThreadItemState::Text(value)
            if value.channel() == ThreadTextChannel::Commentary
                && value.text() == "hello world"
                && matches!(value.lifecycle(), ThreadContentLifecycle::Completed(_))
    ));
    let hot = bus.hot_history("thread-1").unwrap();
    assert_eq!(hot.turns.len(), 1);
    assert_eq!(hot.turns[0].id, "turn-1");
    assert!(matches!(hot.turns[0].state, TurnState::Completed(_)));
    assert_eq!(hot.items, snapshot.items);
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
                        delta: ThreadItemDeltaState::Text {
                            delta: " world".to_string(),
                        },
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
                        state: TurnState::Completed(CompletedTurnState::new(
                            Some(1),
                            3,
                            TurnCompletion::Normal,
                        )),
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
        item: Box::new(commentary_item(
            text,
            0,
            ThreadContentLifecycle::streaming(),
        )),
    }
}

fn commentary_item(text: &str, revision: u64, lifecycle: ThreadContentLifecycle) -> ThreadItem {
    ThreadItem::new(
        "item-1".to_string(),
        "thread-1".to_string(),
        "turn-1".to_string(),
        1,
        revision,
        1,
        1,
        ThreadItemState::Text(ThreadTextItem::new(
            ThreadTextChannel::Commentary,
            text.to_string(),
            Vec::new(),
            lifecycle,
        )),
    )
}

fn active_turn(phase: TurnPhase) -> Turn {
    Turn {
        id: "turn-1".to_string(),
        thread_id: "thread-1".to_string(),
        revision: 0,
        state: TurnState::Running(RunningTurnState::new(1, phase)),
        updated_at: 1,
    }
}

#[tokio::test]
async fn bus_assigns_monotonic_ordinals_and_normalizes_broadcast() {
    // 唯一排序者不变式：新 item 由总线按到达序分配 max+1；更新保留首次
    // 分配值；广播/规范化通知携带最终 ordinal（DB 与订阅者同源）。
    let bus = ThreadEventBus::new(ThreadEventOptions::default());
    bus.replace_snapshot(ThreadSnapshot::empty("thread-1"))
        .unwrap();
    let handle = bus.handle();

    let user_a = notification(
        1,
        ThreadNotification::ItemCompleted {
            item: Box::new(user_message_item("turn-1:user", 0)),
        },
    );
    let user_b = notification(
        2,
        ThreadNotification::ItemCompleted {
            item: Box::new(user_message_item("turn-2:user", 0)),
        },
    );
    let projection = handle
        .project("thread-1", &[user_a, user_b])
        .expect("projection");
    let ordinals = projection
        .notifications
        .iter()
        .filter_map(|envelope| match &envelope.notification {
            ThreadNotification::ItemCompleted { item } => Some(item.ordinal),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(ordinals, vec![1, 2], "user inputs get arrival ordinals");

    // 广播后快照顺序：用户消息按分配序排列。
    handle
        .publish_batch(projection.notifications.clone())
        .await
        .unwrap();
    let snapshot = handle.snapshot("thread-1").unwrap();
    let ids = snapshot
        .items
        .iter()
        .map(|item| item.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["turn-1:user", "turn-2:user"]);

    // 更新已存在 item：载荷 revision 提升但 ordinal 保持首次分配值。
    let updated = notification(
        3,
        ThreadNotification::ItemCompleted {
            item: Box::new(user_message_item("turn-1:user", 99)),
        },
    );
    handle.publish_batch(vec![updated.clone()]).await.unwrap();
    let snapshot = handle.snapshot("thread-1").unwrap();
    let first = snapshot.items.first().unwrap();
    assert_eq!(first.id, "turn-1:user");
    assert_eq!(
        first.ordinal, 1,
        "ordinal is immutable after first assignment"
    );
}

#[tokio::test]
async fn bus_continues_ordinals_after_snapshot_restore() {
    // 恢复续号：replace_snapshot 用 DB ordinal 种子化，后续分配从 max+1 继续。
    let bus = ThreadEventBus::new(ThreadEventOptions::default());
    let mut restored = ThreadSnapshot::empty("thread-1");
    restored.items = vec![
        user_message_item("restored-a", 0),
        user_message_item("restored-b", 7),
    ];
    bus.replace_snapshot(restored).unwrap();
    let handle = bus.handle();

    let fresh = notification(
        1,
        ThreadNotification::ItemCompleted {
            item: Box::new(user_message_item("turn-new:user", 0)),
        },
    );
    handle.publish_batch(vec![fresh]).await.unwrap();
    let snapshot = handle.snapshot("thread-1").unwrap();
    let ordinals = snapshot
        .items
        .iter()
        .map(|item| (item.id.as_str(), item.ordinal))
        .collect::<Vec<_>>();
    assert_eq!(
        ordinals,
        vec![("restored-a", 0), ("restored-b", 7), ("turn-new:user", 8)]
    );
}

fn user_message_item(id: &str, ordinal: u64) -> ThreadItem {
    ThreadItem::new(
        id.to_string(),
        "thread-1".to_string(),
        "turn-1".to_string(),
        ordinal,
        0,
        1,
        1,
        ThreadItemState::Text(ThreadTextItem::new(
            ThreadTextChannel::User,
            format!("message {id}"),
            Vec::new(),
            ThreadContentLifecycle::completed(1),
        )),
    )
}
