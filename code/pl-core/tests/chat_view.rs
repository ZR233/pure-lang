use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use pl_core::chat::{
    ChatError, ChatFocus, ChatHistory, ChatItem, ChatQuery, ChatUpdate, Direction, HistoryPage,
    Session, ViewChange, WeakSession,
};

#[derive(Debug, Default)]
struct Stored {
    items: BTreeMap<u64, ChatItem>,
}

impl ChatHistory for Stored {
    async fn latest_allocated_order(&self) -> Result<u64, ChatError> {
        Ok(self.items.keys().next_back().copied().unwrap_or(0))
    }

    async fn page(&self, query: ChatQuery, limit: usize) -> Result<HistoryPage, ChatError> {
        let items: Vec<_> = self.items.values().cloned().collect();
        let selected: Vec<_> = match query {
            ChatQuery::Latest => items.iter().rev().take(limit).cloned().collect(),
            ChatQuery::Before(order) => items
                .iter()
                .filter(|item| item.order < order)
                .rev()
                .take(limit)
                .cloned()
                .collect(),
            ChatQuery::After(order) => items
                .iter()
                .filter(|item| item.order > order)
                .take(limit)
                .cloned()
                .collect(),
            ChatQuery::Around(order) => {
                let before: Vec<_> = items
                    .iter()
                    .filter(|item| item.order < order)
                    .rev()
                    .take(limit / 2)
                    .cloned()
                    .collect();
                let after = items
                    .iter()
                    .filter(|item| item.order >= order)
                    .take(limit - before.len())
                    .cloned();
                before.into_iter().chain(after).collect()
            }
        };
        let selected = selected.into_iter().collect::<Vec<_>>();
        let oldest = selected.iter().map(|item| item.order).min().unwrap_or(0);
        let newest = selected.iter().map(|item| item.order).max().unwrap_or(0);
        Ok(HistoryPage {
            has_older: self.items.keys().any(|order| *order < oldest),
            has_newer: self.items.keys().any(|order| *order > newest),
            items: selected,
        })
    }

    async fn item(&self, item_id: &str) -> Result<Option<ChatItem>, ChatError> {
        Ok(self
            .items
            .values()
            .find(|item| item.item_id == item_id)
            .cloned())
    }

    async fn read_body(&self, item_id: &str) -> Result<Option<Arc<str>>, ChatError> {
        Ok(self.item(item_id).await?.map(|item| item.body))
    }
}

fn item(order: u64, revision: u64, body: &str, saved: bool) -> ChatItem {
    ChatItem {
        item_id: format!("item-{order}"),
        turn_id: format!("turn-{order}"),
        order,
        revision,
        part_id: Some(format!("part-{order}")),
        body: Arc::from(body),
        omitted_bytes: 0,
        saved,
    }
}

#[tokio::test]
async fn long_streaming_preview_remains_readable_before_commit() {
    let session = Session::new(Stored::default());
    let full = "a".repeat(300 * 1024);
    session.publish_preview(item(1, 1, &full, false)).unwrap();
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    let visible = &view.snapshot().items[0];
    assert!(visible.omitted_bytes > 0);
    assert!(visible.body.len() < full.len());
    assert_eq!(
        view.read_body("item-1").await.unwrap().unwrap().as_ref(),
        full
    );
    assert_eq!(
        view.read_item("item-1")
            .await
            .unwrap()
            .unwrap()
            .body
            .as_ref(),
        full
    );
}

#[tokio::test]
async fn committed_item_replaces_same_revision_speculative_preview() {
    let session = Session::new(Stored::default());
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    session
        .publish_preview(item(1, 7, "streaming", false))
        .unwrap();
    session.publish(item(1, 7, "completed", true)).unwrap();
    let visible = view.snapshot();
    assert_eq!(visible.items.len(), 1);
    assert_eq!(visible.items[0].body.as_ref(), "completed");
    assert!(visible.items[0].saved);
    assert!(matches!(
        session.publish(item(1, 7, "different committed fact", true)),
        Err(ChatError::Conflict(_))
    ));
}

#[tokio::test]
async fn committed_item_replaces_same_revision_pending_projection() {
    let session = Session::new(Stored::default());
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    session.publish(item(1, 7, "pending", false)).unwrap();
    let mut committed = item(1, 7, "canonical", true);
    committed.turn_id = "assigned-at-commit".into();
    session.publish(committed).unwrap();
    let visible = view.snapshot();
    assert_eq!(visible.items.len(), 1);
    assert_eq!(visible.items[0].body.as_ref(), "canonical");
    assert!(visible.items[0].saved);
    assert!(matches!(
        session.publish(item(1, 7, "conflicting committed", true)),
        Err(ChatError::Conflict(_))
    ));
}

#[tokio::test]
async fn paging_merges_uncommitted_rows_without_a_hole_or_a_second_identity() {
    let session = Session::new(Stored {
        items: (1..=120)
            .map(|order| (order, item(order, 1, "stored", true)))
            .collect(),
    });
    for order in 121..=300 {
        session.publish(item(order, 1, "pending", false)).unwrap();
    }

    let view = session
        .open_chat(ChatFocus::Around("item-201".into()))
        .await
        .unwrap();
    let mut orders: Vec<_> = view
        .snapshot()
        .items
        .iter()
        .map(|item| item.order)
        .collect();
    assert!(orders.contains(&201));
    assert!(view.snapshot().has_older);
    assert!(view.snapshot().has_newer);
    let mut crossed_boundary = false;
    for _ in 0..7 {
        let snapshot = view.load(Direction::Older).await.unwrap();
        orders = snapshot.items.iter().map(|item| item.order).collect();
        assert!(orders.len() <= 96);
        assert!(orders.windows(2).all(|pair| pair[0] + 1 == pair[1]));
        assert!(snapshot.has_newer);
        crossed_boundary |= orders.contains(&120) && orders.contains(&121);
    }
    assert!(crossed_boundary);
    assert_eq!(
        orders.len(),
        orders
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
    );
}

#[tokio::test]
async fn old_window_stays_put_and_slow_tail_subscriber_resets() {
    let session = Session::new(Stored::default());
    session.publish(item(1, 1, "a", false)).unwrap();
    let latest = session.open_chat(ChatFocus::Latest).await.unwrap();
    let old = session
        .open_chat(ChatFocus::Around("item-1".into()))
        .await
        .unwrap();
    let (initial, mut updates) = latest.subscribe();
    assert_eq!(initial.items.len(), 1);
    session.publish(item(2, 1, "b", false)).unwrap();
    assert!(matches!(
        updates.next().await,
        Some(ChatUpdate::Patch { .. })
    ));
    for order in 3..=200 {
        session.publish(item(order, 1, "chunk", false)).unwrap();
    }
    assert!(matches!(updates.next().await, Some(ChatUpdate::Reset(_))));
    let old = old.snapshot();
    assert_eq!(old.items.len(), 1);
    assert_eq!(old.items[0].order, 1);
    assert!(old.has_newer);
    assert_eq!(latest.snapshot().items.last().unwrap().order, 200);
}

#[tokio::test]
async fn latest_window_stays_at_initial_page_until_user_loads_older() {
    let session = Session::new(Stored::default());
    for order in 1..=100 {
        session.publish(item(order, 1, "recent", false)).unwrap();
    }
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    let (initial, mut updates) = view.subscribe();
    assert_eq!(initial.items.len(), 32);

    for order in 101..=300 {
        session.publish(item(order, 1, "new", false)).unwrap();
    }
    let update = updates.next().await.unwrap();
    assert!(matches!(update, ChatUpdate::Reset(_)));
    let snapshot = view.snapshot();
    assert_eq!(snapshot.items.len(), 32);
    assert_eq!(snapshot.items.first().unwrap().order, 269);
    assert_eq!(snapshot.items.last().unwrap().order, 300);
    let first_page = view.load(Direction::Older).await.unwrap();
    assert_eq!(first_page.items.len(), 64);
    let second_page = view.load(Direction::Older).await.unwrap();
    assert_eq!(second_page.items.len(), 96);
    assert_eq!(second_page.items.first().unwrap().order, 205);
    assert_eq!(second_page.items.last().unwrap().order, 300);
}

#[tokio::test]
async fn newer_revision_keeps_save_responsibility_and_emits_text_append() {
    let session = Session::new(Stored::default());
    session.publish(item(1, 10, "hello", false)).unwrap();
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    let (_, mut updates) = view.subscribe();
    session.publish(item(1, 11, "hello world", false)).unwrap();
    assert!(
        matches!(updates.next().await, Some(ChatUpdate::Patch { changes, .. })
        if matches!(&changes[..], [ViewChange::AppendText { expected_revision: 10, revision: 11, text, .. }] if text == " world"))
    );
    session.confirm_saved("item-1", 10);
    assert!(!view.snapshot().items[0].saved);
    session.confirm_saved("item-1", 11);
    assert!(view.snapshot().items[0].saved);
}

#[tokio::test]
async fn accepted_message_can_bind_to_its_turn_without_changing_identity() {
    let session = Session::new(Stored::default());
    let mut accepted = item(1, 1, "submitted", false);
    accepted.turn_id.clear();
    session.publish(accepted).unwrap();
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();

    session.publish(item(1, 2, "submitted", false)).unwrap();
    let snapshot = view.snapshot();
    assert_eq!(snapshot.items.len(), 1);
    assert_eq!(snapshot.items[0].item_id, "item-1");
    assert_eq!(snapshot.items[0].turn_id, "turn-1");
    assert!(!snapshot.items[0].saved);
}

#[tokio::test]
async fn different_items_cannot_overwrite_one_unsaved_order() {
    let session = Session::new(Stored::default());
    session.publish(item(1, 1, "first", false)).unwrap();
    let mut conflicting = item(1, 1, "second", false);
    conflicting.item_id = "another-item".into();
    assert!(matches!(
        session.publish(conflicting),
        Err(ChatError::Conflict(id)) if id == "another-item"
    ));
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    let snapshot = view.snapshot();
    assert_eq!(snapshot.items.len(), 1);
    assert_eq!(snapshot.items[0].item_id, "item-1");
    assert_eq!(snapshot.items[0].body.as_ref(), "first");
}

#[tokio::test]
async fn preview_orders_survive_tail_eviction_until_terminal_admission() {
    let session = Session::new(Stored::default());
    session.initialize_order_allocator().await.unwrap();
    for index in 1..=200 {
        let id = format!("item-{index}");
        let order = session.reserve_order_in_memory(&id).unwrap();
        assert_eq!(order, index);
        session
            .publish_preview(item(order, 1, "preview", false))
            .unwrap();
    }
    assert_eq!(session.assigned_order("item-1"), Some(1));
    assert_eq!(session.assigned_order("item-200"), Some(200));

    session.publish(item(1, 2, "complete", false)).unwrap();
    session.drop_previews_with_prefix("item-");
    assert_eq!(session.assigned_order("item-1"), Some(1));
    assert_eq!(session.assigned_order("item-200"), None);
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    let first = view.read_item("item-1").await.unwrap().unwrap();
    assert_eq!(first.body.as_ref(), "complete");
    assert!(!first.saved);
}

#[derive(Debug)]
struct UnavailableHistory;

impl ChatHistory for UnavailableHistory {
    async fn latest_allocated_order(&self) -> Result<u64, ChatError> {
        panic!("a warm view does not need to seed a new order")
    }

    async fn page(&self, _: ChatQuery, _: usize) -> Result<HistoryPage, ChatError> {
        panic!("warm first frame must not query SQLite");
    }

    async fn item(&self, _: &str) -> Result<Option<ChatItem>, ChatError> {
        panic!("warm first frame must not resolve an anchor");
    }

    async fn read_body(&self, _: &str) -> Result<Option<Arc<str>>, ChatError> {
        panic!("warm first frame must not read a body");
    }
}

#[tokio::test]
async fn warm_tail_opens_a_bounded_first_frame_without_database_io() {
    let session = Session::new(UnavailableHistory);
    for order in 1..=120 {
        session.publish(item(order, 1, "recent", false)).unwrap();
    }
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    let (initial, _) = view.subscribe();
    assert_eq!(initial.items.len(), 32);
    assert_eq!(initial.items.first().unwrap().order, 89);
    assert_eq!(initial.items.last().unwrap().order, 120);
    assert!(initial.has_older);
}

#[tokio::test]
async fn one_identity_from_memory_and_database_is_never_shown_twice() {
    let session = Session::new(Stored {
        items: [(1, item(1, 1, "old", true))].into(),
    });
    let mut replacement = item(2, 2, "new", false);
    replacement.item_id = "item-1".into();
    session.publish(replacement).unwrap();
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    let snapshot = view.snapshot();
    assert_eq!(snapshot.items.len(), 1);
    assert_eq!(snapshot.items[0].item_id, "item-1");
    assert_eq!(snapshot.items[0].body.as_ref(), "new");
    assert!(!snapshot.items[0].saved);
}

#[derive(Debug, Default)]
struct StaleReadAfterCommit {
    session: Arc<Mutex<Option<WeakSession>>>,
}

impl ChatHistory for StaleReadAfterCommit {
    async fn latest_allocated_order(&self) -> Result<u64, ChatError> {
        Ok(0)
    }

    async fn page(&self, query: ChatQuery, _: usize) -> Result<HistoryPage, ChatError> {
        if matches!(query, ChatQuery::Before(185))
            && let Some(session) = self
                .session
                .lock()
                .unwrap()
                .as_ref()
                .and_then(WeakSession::upgrade)
        {
            session.confirm_saved("item-160", 1);
        }
        Ok(HistoryPage {
            items: Vec::new(),
            has_older: true,
            has_newer: true,
        })
    }

    async fn item(&self, _: &str) -> Result<Option<ChatItem>, ChatError> {
        Ok(None)
    }

    async fn read_body(&self, _: &str) -> Result<Option<Arc<str>>, ChatError> {
        Ok(None)
    }
}

#[derive(Debug)]
struct AllocatedHistory {
    maximum: u64,
    reads: Arc<AtomicUsize>,
}

impl ChatHistory for AllocatedHistory {
    async fn latest_allocated_order(&self) -> Result<u64, ChatError> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        Ok(self.maximum)
    }

    async fn page(&self, _: ChatQuery, _: usize) -> Result<HistoryPage, ChatError> {
        Ok(HistoryPage {
            items: Vec::new(),
            has_older: false,
            has_newer: false,
        })
    }

    async fn item(&self, _: &str) -> Result<Option<ChatItem>, ChatError> {
        Ok(None)
    }

    async fn read_body(&self, _: &str) -> Result<Option<Arc<str>>, ChatError> {
        Ok(None)
    }
}

#[tokio::test]
async fn cold_order_seed_is_read_once_and_hot_allocations_do_not_touch_storage() {
    let reads = Arc::new(AtomicUsize::new(0));
    let session = Session::new(AllocatedHistory {
        maximum: 900,
        reads: reads.clone(),
    });
    assert!(matches!(
        session.reserve_order_in_memory("first"),
        Err(ChatError::OrderNotInitialized)
    ));
    session.initialize_order_allocator().await.unwrap();
    assert_eq!(session.reserve_order_in_memory("first").unwrap(), 901);
    assert_eq!(session.reserve_order("first").await.unwrap(), 901);
    assert_eq!(session.reserve_order_in_memory("second").unwrap(), 902);
    assert_eq!(reads.load(Ordering::Relaxed), 1);

    let mut first = item(901, 1, "accepted", false);
    first.item_id = "first".into();
    session.publish(first).unwrap();
    assert_eq!(session.reserve_order("first").await.unwrap(), 901);
    assert_eq!(reads.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn concurrent_order_reservations_continue_after_a_newer_in_memory_record() {
    let session = Session::new(AllocatedHistory {
        maximum: 900,
        reads: Arc::new(AtomicUsize::new(0)),
    });
    session.publish(item(950, 1, "pending", false)).unwrap();
    let orders = futures::future::join_all((0..100).map(|index| {
        let session = session.clone();
        async move {
            session
                .reserve_order(&format!("new-{index}"))
                .await
                .unwrap()
        }
    }))
    .await;
    let distinct = orders
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(distinct.len(), 100);
    assert_eq!(distinct.first().copied(), Some(951));
    assert_eq!(distinct.last().copied(), Some(1050));
}

#[tokio::test]
async fn paging_keeps_a_record_when_writer_acks_after_a_stale_database_read() {
    let history = StaleReadAfterCommit::default();
    let registry = history.session.clone();
    let session = Session::new(history);
    *registry.lock().unwrap() = Some(session.downgrade());
    for order in 1..=300 {
        session.publish(item(order, 1, "pending", false)).unwrap();
    }
    let view = session
        .open_chat(ChatFocus::Around("item-201".into()))
        .await
        .unwrap();
    assert_eq!(view.snapshot().items.first().unwrap().order, 185);
    let page = view.load(Direction::Older).await.unwrap();
    let orders: Vec<_> = page.items.iter().map(|item| item.order).collect();
    assert!(orders.contains(&160), "committed item vanished: {orders:?}");
    assert!(orders.windows(2).all(|pair| pair[0] + 1 == pair[1]));
}
