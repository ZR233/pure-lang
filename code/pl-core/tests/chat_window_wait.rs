//! The long-poll window wait, expressed through the public chat API.
//!
//! A consumer parks one `next` on its own window handle while another handle on the same shared
//! session moves the window. Two properties must hold, because every transport (FRB 窗口 and HTTP
//! window subscriptions) is built on them:
//!
//! 1. the parked wait must not block the move — the wait never holds the window across its await,
//!    so a move on another handle still completes;
//! 2. the move must not be lost by the parked wait — it is delivered from the consumer's own
//!    baseline instead of being silently skipped.
//!
//! Re-subscribing after a move must start from the moved window, so an authoritative baseline
//! reset always yields a frame the consumer can continue from.

use std::{collections::BTreeMap, sync::Arc, time::Duration};

use pl_core::chat::{
    ChatError, ChatField, ChatFocus, ChatHistory, ChatItem, ChatLifecycle, ChatQuery, ChatUpdate,
    Direction, HistoryPage, Session,
};
use pl_core::model::ContentBlock;

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
}

/// One item whose primary body field carries `text`.
fn item(order: u64, revision: u64, text: &str) -> ChatItem {
    ChatItem {
        item_id: format!("item-{order}"),
        turn_id: format!("turn-{order}"),
        order,
        revision,
        fields: BTreeMap::from([(ChatField::Body, ContentBlock::from_text(text))]),
        meta: Arc::from(Vec::<u8>::new()),
        omitted_bytes: 0,
        saved: false,
        lifecycle: ChatLifecycle::Streaming,
    }
}

fn stored_session(count: u64) -> Session {
    let session = Session::new(Stored::default());
    for order in 1..=count {
        session
            .publish(item(order, 1, "recent"))
            .expect("stored history accepts one item per order");
    }
    session
}

/// A parked wait and a window move run concurrently on two handles of the same session.
#[tokio::test]
async fn a_parked_wait_is_neither_blocked_nor_lost_by_a_window_move() {
    let session = stored_session(100);
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    let (initial, mut updates) = view.subscribe();
    assert_eq!(initial.items.len(), 32);

    // Both futures are driven in one task: if the parked wait held the window across its await, the
    // move on the other handle would never complete and this would time out.
    let (frame, page) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(updates.next(), view.load(Direction::Older))
    })
    .await
    .expect("a parked wait must not block a window move on another handle");

    let page = page.expect("paging older must succeed");
    assert_eq!(
        page.items.len(),
        64,
        "older paging must still make progress"
    );
    let frame = frame.expect("the parked wait must not lose the move's notification");
    match frame {
        ChatUpdate::Patch { from, to, .. } => {
            // A patch is diffed against this consumer's own baseline, never against the move.
            assert_eq!(from, initial.version);
            assert!(to >= from);
        }
        ChatUpdate::Reset(window) => {
            // The move changed the focus, which the patch vocabulary cannot express; the
            // authoritative window is the same one the handle now reports.
            assert_eq!(window.version, view.snapshot().version);
        }
    }
}

/// A fresh subscription is the only authoritative baseline reset: the frame it reports is the base
/// the following patches continue from.
#[tokio::test]
async fn resubscribing_after_a_move_reports_the_frame_patches_continue_from() {
    let session = stored_session(100);
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    view.load(Direction::Older).await.unwrap();

    let (baseline, mut updates) = view.subscribe();
    assert_eq!(baseline.version, view.snapshot().version);

    session.publish(item(101, 1, "tail")).unwrap();
    let frame = tokio::time::timeout(Duration::from_secs(5), updates.next())
        .await
        .expect("a change after a re-subscription must be delivered")
        .expect("the session still has changes");
    if let ChatUpdate::Patch { from, to, .. } = frame {
        assert_eq!(from, baseline.version);
        assert!(to >= from);
    }
}
