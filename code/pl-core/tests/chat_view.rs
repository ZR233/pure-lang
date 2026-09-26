use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use pl_core::{
    chat::{
        ChatError, ChatField, ChatFocus, ChatHistory, ChatItem, ChatLifecycle, ChatQuery,
        ChatSnapshot, ChatUpdate, ChatUpdatePriority, ChatUpdates, ChatView, Direction,
        FieldChange, FieldUpdate, HistoryPage, PresentationPart, Session, ViewChange, WeakSession,
    },
    model::ContentBlock,
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
}

/// Counts complete-by-identity reads so a test can prove the window reads durable history once.
#[derive(Debug)]
struct Counting {
    inner: Stored,
    item_reads: Arc<AtomicUsize>,
}

impl ChatHistory for Counting {
    async fn latest_allocated_order(&self) -> Result<u64, ChatError> {
        self.inner.latest_allocated_order().await
    }

    async fn page(&self, query: ChatQuery, limit: usize) -> Result<HistoryPage, ChatError> {
        self.inner.page(query, limit).await
    }

    async fn item(&self, item_id: &str) -> Result<Option<ChatItem>, ChatError> {
        self.item_reads.fetch_add(1, Ordering::Relaxed);
        // Yield so two concurrent callers genuinely interleave before either can retain the body.
        tokio::task::yield_now().await;
        self.inner.item(item_id).await
    }
}

/// A history whose stored items can be mutated between calls, to model storage advancing (or lagging)
/// behind the live window while a `read_complete` is in flight.
#[derive(Debug, Clone, Default)]
struct Mutable {
    items: Arc<Mutex<BTreeMap<u64, ChatItem>>>,
}

impl Mutable {
    fn set(&self, item: ChatItem) {
        self.items.lock().unwrap().insert(item.order, item);
    }
}

impl ChatHistory for Mutable {
    async fn latest_allocated_order(&self) -> Result<u64, ChatError> {
        Ok(self
            .items
            .lock()
            .unwrap()
            .keys()
            .next_back()
            .copied()
            .unwrap_or(0))
    }

    async fn page(&self, query: ChatQuery, limit: usize) -> Result<HistoryPage, ChatError> {
        // Clone out of the lock; no guard is held across the (test-local) await.
        let items = self.items.lock().unwrap().clone();
        Stored { items }.page(query, limit).await
    }

    async fn item(&self, item_id: &str) -> Result<Option<ChatItem>, ChatError> {
        Ok(self
            .items
            .lock()
            .unwrap()
            .values()
            .find(|item| item.item_id == item_id)
            .cloned())
    }
}

fn empty_meta() -> Arc<[u8]> {
    Arc::from(Vec::<u8>::new())
}

/// One item whose primary body field carries `text`.
fn item(order: u64, revision: u64, text: &str, terminal: bool) -> ChatItem {
    ChatItem {
        item_id: format!("item-{order}"),
        turn_id: format!("turn-{order}"),
        order,
        revision,
        fields: BTreeMap::from([(ChatField::Body, ContentBlock::from_text(text))]),
        meta: empty_meta(),
        omitted_bytes: 0,
        saved: false,
        lifecycle: if terminal {
            ChatLifecycle::Terminal
        } else {
            ChatLifecycle::Streaming
        },
    }
}

/// The same item marked as acknowledged durable history, exercising the save watermark on its own.
fn durable(mut item: ChatItem) -> ChatItem {
    item.saved = true;
    item
}

/// One item with an explicit identity, so a test can name a provider presentation row exactly as the
/// runtime projection does instead of the synthetic `item-{order}` identity.
fn identified(item_id: &str, order: u64, revision: u64, text: &str, terminal: bool) -> ChatItem {
    let mut item = item(order, revision, text, terminal);
    item.item_id = item_id.to_owned();
    item
}

fn body_text(item: &ChatItem) -> String {
    item.body().map(|block| block.text()).unwrap_or_default()
}

/// The same item with every content field extended by `delta` along its shared block chain.
///
/// This is how the producer publishes a delta: the new revision shares the previous block instead of
/// materializing the whole body again.
fn appended(previous: &ChatItem, revision: u64, delta: &str) -> ChatItem {
    let mut next = previous.clone();
    next.revision = revision;
    for block in next.fields.values_mut() {
        let extended = ContentBlock::append(&*block, delta);
        *block = extended;
    }
    next
}

/// Minimal canonical client: the rows a host reduces real `ChatUpdate` frames into.
///
/// Applying the batch exactly as a host would, then comparing with the authoritative window, proves
/// the patch vocabulary is complete and applicable instead of only matching an enum shape.
#[derive(Clone, Debug)]
struct ClientRow {
    item_id: String,
    order: u64,
    turn_id: String,
    revision: u64,
    omitted_bytes: u64,
    saved: bool,
    lifecycle: ChatLifecycle,
    meta: Arc<[u8]>,
    fields: BTreeMap<ChatField, Arc<ContentBlock>>,
}

impl ClientRow {
    fn from_item(item: &ChatItem) -> Self {
        Self {
            item_id: item.item_id.clone(),
            order: item.order,
            turn_id: item.turn_id.clone(),
            revision: item.revision,
            omitted_bytes: item.omitted_bytes,
            saved: item.saved,
            lifecycle: item.lifecycle,
            meta: item.meta.clone(),
            fields: item.fields.clone(),
        }
    }
}

#[derive(Clone, Debug)]
struct Client {
    rows: Vec<ClientRow>,
}

impl Client {
    fn new(snapshot: &ChatSnapshot) -> Self {
        Self {
            rows: snapshot.items.iter().map(ClientRow::from_item).collect(),
        }
    }

    fn apply(&mut self, update: &ChatUpdate) {
        match update {
            ChatUpdate::Reset(snapshot) => {
                self.rows = snapshot.items.iter().map(ClientRow::from_item).collect();
            }
            ChatUpdate::Patch { changes, .. } => {
                for change in changes {
                    match change {
                        ViewChange::Splice {
                            index,
                            remove,
                            items,
                        } => {
                            let rows = items.iter().map(ClientRow::from_item);
                            self.rows.splice(*index..*index + *remove, rows);
                        }
                        ViewChange::UpdateItem {
                            item_id,
                            expected_revision,
                            revision,
                            omitted_bytes,
                            saved,
                            fields,
                        } => {
                            let row = self
                                .rows
                                .iter_mut()
                                .find(|row| &row.item_id == item_id)
                                .unwrap_or_else(|| panic!("update for unknown identity {item_id}"));
                            // One baseline check for the whole batch, then one version commit.
                            assert_eq!(
                                row.revision, *expected_revision,
                                "update baseline must match the delivered revision"
                            );
                            for delta in fields {
                                match &delta.change {
                                    FieldChange::Remove => {
                                        row.fields.remove(&delta.field);
                                    }
                                    FieldChange::Unchanged => {}
                                    FieldChange::Append(text) => {
                                        let block =
                                            row.fields.get(&delta.field).unwrap_or_else(|| {
                                                panic!("append for unknown field {:?}", delta.field)
                                            });
                                        let extended = ContentBlock::append(block, text);
                                        row.fields.insert(delta.field.clone(), extended);
                                    }
                                    FieldChange::Replace(block) => {
                                        row.fields.insert(delta.field.clone(), block.clone());
                                    }
                                }
                            }
                            row.revision = *revision;
                            row.omitted_bytes = *omitted_bytes;
                            row.saved = *saved;
                        }
                    }
                }
            }
        }
    }

    fn assert_matches(&self, snapshot: &ChatSnapshot) {
        assert_eq!(self.rows.len(), snapshot.items.len(), "row count");
        for (row, item) in self.rows.iter().zip(&snapshot.items) {
            assert_eq!(row.item_id, item.item_id, "identity");
            assert_eq!(row.order, item.order, "order");
            assert_eq!(row.turn_id, item.turn_id, "turn");
            assert_eq!(row.revision, item.revision, "content revision");
            assert_eq!(row.omitted_bytes, item.omitted_bytes, "omitted bytes");
            assert_eq!(row.saved, item.saved, "save watermark");
            assert_eq!(row.lifecycle, item.lifecycle, "lifecycle");
            assert_eq!(row.meta, item.meta, "meta");
            assert_eq!(row.fields.len(), item.fields.len(), "field count");
            for (field, block) in &item.fields {
                let client = row
                    .fields
                    .get(field)
                    .unwrap_or_else(|| panic!("field {field:?} missing on the client"));
                assert_eq!(client.text(), block.text(), "field {field:?} content");
            }
        }
    }
}

/// Delivers one frame, applies it through the client reducer and checks it against the window.
async fn apply_next(client: &mut Client, updates: &mut ChatUpdates, view: &ChatView) -> ChatUpdate {
    let update = updates.next().await.expect("an update frame");
    client.apply(&update);
    client.assert_matches(&view.snapshot());
    update
}

fn expect_update(update: &ChatUpdate) -> (&str, u64, u64, u64, &[FieldUpdate]) {
    match update {
        ChatUpdate::Patch { changes, .. } => match &changes[..] {
            [
                ViewChange::UpdateItem {
                    item_id,
                    expected_revision,
                    revision,
                    omitted_bytes,
                    saved: _,
                    fields,
                },
            ] => (
                item_id.as_str(),
                *expected_revision,
                *revision,
                *omitted_bytes,
                fields.as_slice(),
            ),
            other => panic!("expected one batched item update, got {other:?}"),
        },
        other => panic!("expected a patch, got {other:?}"),
    }
}

#[tokio::test]
async fn in_flight_body_is_complete_in_the_window_and_streams_along_it() {
    let session = Session::new(Stored::default());
    let full = "a".repeat(300 * 1024);
    let first = item(1, 1, &full, false);
    session.publish_preview(first.clone()).unwrap();
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    let visible = &view.snapshot().items[0];
    // The in-flight body is served complete: a live preview is never silently truncated.
    assert_eq!(visible.omitted_bytes, 0);
    assert_eq!(body_text(visible), full);
    assert_eq!(
        body_text(&view.read_complete("item-1").await.unwrap().unwrap()),
        full
    );
    // Later deltas extend that same complete body instead of restarting from a bounded prefix.
    let (initial, mut updates) = view.subscribe();
    let mut client = Client::new(&initial);
    session
        .publish_preview(appended(&first, 2, "tail"))
        .unwrap();
    let update = apply_next(&mut client, &mut updates, &view).await;
    assert_eq!(update.priority(), ChatUpdatePriority::Coalesced);
    let (item_id, expected, revision, omitted, fields) = expect_update(&update);
    assert_eq!(item_id, "item-1");
    assert_eq!(expected, 1);
    assert_eq!(revision, 2);
    assert_eq!(omitted, 0);
    assert!(matches!(
        fields,
        [FieldUpdate {
            field: ChatField::Body,
            change: FieldChange::Append(text),
        }] if text == "tail"
    ));
}

#[tokio::test]
async fn complete_body_is_read_from_durable_history_once() {
    let full = "b".repeat(300 * 1024);
    let reads = Arc::new(AtomicUsize::new(0));
    let session = Session::new(Counting {
        inner: Stored {
            items: [(1, item(1, 1, &full, true))].into(),
        },
        item_reads: reads.clone(),
    });
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    // A committed item keeps a bounded preview in the window so its resident bytes stay small.
    assert!(view.snapshot().items[0].omitted_bytes > 0);
    let first = view.read_complete("item-1").await.unwrap().unwrap();
    assert_eq!(body_text(&first), full);
    // The complete body is retained for this window instead of rereading SQLite.
    let second = view.read_complete("item-1").await.unwrap().unwrap();
    assert_eq!(body_text(&second), full);
    assert_eq!(reads.load(Ordering::Relaxed), 1);
    // A later frame carries the complete body already served to this window.
    assert_eq!(body_text(&view.snapshot().items[0]), full);
}

/// A view that requested a large in-flight body keeps following it across later revisions.
///
/// A durable but still-streaming body larger than the preview bound is shown as a bounded preview
/// until the host asks for the complete body by identity. From then on the window must keep the
/// standing complete-body intent: each later revision is delivered as an append along the same
/// shared chain, the visible body never falls back to a preview, and no further storage read happens
/// while the identity stays in the window. The real client reducer proves the frames are applicable,
/// not just enum-shaped. The intent is released with the identity, so a request after it leaves the
/// window reads storage again instead of serving the released copy.
#[tokio::test]
async fn a_requested_complete_body_keeps_following_new_revisions_without_store_reads() {
    let full = "g".repeat(300 * 1024);
    let reads = Arc::new(AtomicUsize::new(0));
    // Saved and still streaming: durable history owns the body, so the window shows a bounded preview
    // until the host asks for the complete body by identity.
    let mut base = durable(item(1, 1, &full, false));
    let session = Session::new(Counting {
        inner: Stored {
            items: [(1, base.clone())].into(),
        },
        item_reads: reads.clone(),
    });
    session.publish(base.clone()).unwrap();
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    assert!(
        view.snapshot().items[0].omitted_bytes > 0,
        "a durable large body previews until the host asks for the complete body"
    );

    // One request upgrades the visible body and establishes the standing complete-body intent.
    let complete = view.read_complete("item-1").await.unwrap().unwrap();
    assert_eq!(complete.omitted_bytes, 0);
    assert_eq!(body_text(&complete), full);
    assert_eq!(reads.load(Ordering::Relaxed), 1);

    let (initial, mut updates) = view.subscribe();
    let mut client = Client::new(&initial);
    let mut expected = full.clone();
    for (revision, delta) in [(2_u64, "second"), (3_u64, "third")] {
        let next = durable(appended(&base, revision, delta));
        session.publish(next.clone()).unwrap();
        base = next;
        expected.push_str(delta);

        let update = apply_next(&mut client, &mut updates, &view).await;
        let (item_id, baseline, delivered, omitted, fields) = expect_update(&update);
        assert_eq!(item_id, "item-1");
        assert_eq!(
            baseline,
            revision - 1,
            "the patch continues the delivered revision"
        );
        assert_eq!(delivered, revision);
        assert_eq!(
            omitted, 0,
            "a followed identity never falls back to a bounded preview"
        );
        assert!(
            matches!(
                fields,
                [FieldUpdate {
                    field: ChatField::Body,
                    change: FieldChange::Append(text),
                }] if text == delta
            ),
            "each new revision extends the delivered complete body: {fields:?}"
        );
        let snapshot = view.snapshot();
        assert_eq!(snapshot.items[0].omitted_bytes, 0);
        assert_eq!(body_text(&snapshot.items[0]), expected);
    }
    assert_eq!(
        reads.load(Ordering::Relaxed),
        1,
        "following a revision never performs another storage read"
    );

    // The intent is released with the identity: once it leaves the window a fresh request reads
    // storage again instead of serving the released copy, and it never resurrects it into the window.
    for order in 2..=300 {
        session.publish(item(order, 1, "tail", false)).unwrap();
    }
    assert!(
        view.snapshot()
            .items
            .iter()
            .all(|item| item.item_id != "item-1"),
        "the followed identity left the window"
    );
    view.read_complete("item-1").await.unwrap().unwrap();
    assert_eq!(reads.load(Ordering::Relaxed), 2);
    assert!(
        view.snapshot()
            .items
            .iter()
            .all(|item| item.item_id != "item-1")
    );
}

#[tokio::test]
async fn complete_body_is_released_once_its_identity_leaves_the_window() {
    let full = "c".repeat(300 * 1024);
    let reads = Arc::new(AtomicUsize::new(0));
    let session = Session::new(Counting {
        inner: Stored {
            items: [(1, item(1, 1, &full, true))].into(),
        },
        item_reads: reads.clone(),
    });
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    view.read_complete("item-1").await.unwrap().unwrap();
    assert_eq!(reads.load(Ordering::Relaxed), 1);
    // The identity leaves the window; the retained complete body must be released with it.
    for order in 2..=300 {
        session.publish(item(order, 1, "tail", false)).unwrap();
    }
    assert!(
        view.snapshot()
            .items
            .iter()
            .all(|item| item.item_id != "item-1"),
        "the identity must have left the window"
    );
    // A new request for the released identity reads storage again instead of serving a released copy.
    view.read_complete("item-1").await.unwrap().unwrap();
    assert_eq!(reads.load(Ordering::Relaxed), 2);
    // ...and the late body still does not resurrect the identity into the window.
    assert!(
        view.snapshot()
            .items
            .iter()
            .all(|item| item.item_id != "item-1")
    );
}

#[tokio::test]
async fn read_complete_does_not_resurrect_a_cancelled_identity() {
    let session = Session::new(Stored::default());
    session
        .publish_preview(item(1, 1, "in-flight", false))
        .unwrap();
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    assert_eq!(view.snapshot().items.len(), 1);
    // The request is cancelled: the speculative identity leaves every window.
    session.drop_preview("item-1");
    assert!(view.snapshot().items.is_empty());
    // A late read for the cancelled identity must not bring it back.
    assert!(view.read_complete("item-1").await.unwrap().is_none());
    assert!(view.snapshot().items.is_empty());
}

#[tokio::test]
async fn concurrent_read_complete_calls_do_not_corrupt_the_window() {
    let full = "d".repeat(300 * 1024);
    let reads = Arc::new(AtomicUsize::new(0));
    let session = Session::new(Counting {
        inner: Stored {
            items: [(1, item(1, 1, &full, true))].into(),
        },
        item_reads: reads.clone(),
    });
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    let (left, right) =
        futures::future::join(view.read_complete("item-1"), view.read_complete("item-1")).await;
    assert_eq!(body_text(&left.unwrap().unwrap()), full);
    assert_eq!(body_text(&right.unwrap().unwrap()), full);
    // The window still serves the complete body once and only once, without a duplicate identity.
    let snapshot = view.snapshot();
    assert_eq!(snapshot.items.len(), 1);
    assert_eq!(body_text(&snapshot.items[0]), full);
    // A race may read storage twice before the cache fills, but never under a lock and never a third
    // time once retained.
    let after_join = reads.load(Ordering::Relaxed);
    assert!(after_join <= 2);
    assert_eq!(
        body_text(&view.read_complete("item-1").await.unwrap().unwrap()),
        full
    );
    assert_eq!(reads.load(Ordering::Relaxed), after_join);
}

/// A late complete-body read whose revision is older than the window's must not return a mixed item.
///
/// The window advanced to revision 2 (a live update) while a `read_complete` was resolving a body
/// against durable history, which still only had revision 1. Copying the newer slot's `saved` /
/// `lifecycle` / metadata onto the older body would return a cross-revision item; the read must
/// instead yield the authoritative newest item and leave the window untouched.
#[tokio::test]
async fn a_late_read_older_than_the_window_never_mixes_revisions() {
    let full_v1 = "v1".repeat(200 * 1024);
    let session = Session::new(Stored {
        items: [(1, item(1, 1, &full_v1, true))].into(),
    });
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    // The host reads the complete revision 1 body; the window upgrades in place at the same revision.
    let complete = view.read_complete("item-1").await.unwrap().unwrap();
    assert_eq!(complete.revision, 1);
    assert_eq!(body_text(&complete), full_v1);

    // A live update advances the window to revision 2 while durable history still holds revision 1.
    let full_v2 = "v2".repeat(200 * 1024);
    session
        .publish(durable(item(1, 2, &full_v2, false)))
        .unwrap();
    assert_eq!(view.snapshot().items[0].revision, 2);
    let version = view.version();

    // A fresh read of the identity resolves the stale revision 1 body from storage; the result must be
    // the window's revision 2 item, never a revision 1 body carrying revision 2's facts.
    let late = view.read_complete("item-1").await.unwrap().unwrap();
    assert_eq!(
        late.revision, 2,
        "a stale read must not be returned as current"
    );
    assert_eq!(late.saved, view.snapshot().items[0].saved);
    assert_eq!(late.lifecycle, view.snapshot().items[0].lifecycle);
    assert_ne!(body_text(&late), full_v1);
    // The late read neither wrote the stale body back into the window nor advanced the version.
    assert_eq!(view.snapshot().items[0].revision, 2);
    assert_eq!(view.version(), version);
}

/// Once a newer complete body is retained, a later stale read must not downgrade what the window
/// serves, and the cache-hit path must answer without re-reading storage or re-locking to a deadlock.
#[tokio::test]
async fn a_stale_read_does_not_overwrite_a_newer_cached_body() {
    let full_v1 = "s1".repeat(200 * 1024);
    let full_v2 = "s2".repeat(200 * 1024);
    let history = Mutable::default();
    history.set(durable(item(1, 1, &full_v1, true)));
    let session = Session::new(history.clone());
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();

    // Storage already holds revision 2 while the window slot is still revision 1; the request retains
    // the newer body on its own facts and returns it (cache is empty, so storage is read once).
    history.set(durable(item(1, 2, &full_v2, true)));
    let newer = view.read_complete("item-1").await.unwrap().unwrap();
    assert_eq!(newer.revision, 2);
    assert_eq!(body_text(&newer), full_v2);

    // A late duplicate of the older revision arrives at storage. The cache-hit path serves the
    // retained revision 2 body and does not overwrite it with the stale revision 1 body.
    history.set(durable(item(1, 1, &full_v1, true)));
    let served = view.read_complete("item-1").await.unwrap().unwrap();
    assert_eq!(served.revision, 2);
    assert_eq!(body_text(&served), full_v2);
}

#[tokio::test]
async fn authoritative_body_replacement_delivers_the_whole_block() {
    let session = Session::new(Stored::default());
    session
        .publish_preview(item(1, 1, "observed", false))
        .unwrap();
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    let (initial, mut updates) = view.subscribe();
    let mut client = Client::new(&initial);
    // The authoritative body no longer extends the delivered prefix, so the consumer must replace
    // the field instead of appending the new text onto a stale body.
    session
        .publish_preview(item(1, 2, "replaced", false))
        .unwrap();
    let update = apply_next(&mut client, &mut updates, &view).await;
    assert_eq!(update.priority(), ChatUpdatePriority::Immediate);
    let (_, _, _, _, fields) = expect_update(&update);
    match fields {
        [
            FieldUpdate {
                change: FieldChange::Replace(block),
                ..
            },
        ] => assert_eq!(block.text(), "replaced"),
        other => panic!("expected a whole-block replacement, got {other:?}"),
    }
}

#[tokio::test]
async fn same_channel_blocks_append_without_overwriting_each_other() {
    let session = Session::new(Stored::default());
    let first = item(1, 1, "first", false);
    let second = item(2, 1, "second", false);
    session.publish(first.clone()).unwrap();
    session.publish(second.clone()).unwrap();
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    let (initial, mut updates) = view.subscribe();
    let mut client = Client::new(&initial);
    session.publish(appended(&first, 2, "-A")).unwrap();
    session.publish(appended(&second, 2, "-B")).unwrap();
    let update = apply_next(&mut client, &mut updates, &view).await;
    let ChatUpdate::Patch { changes, .. } = &update else {
        panic!("two in-flight blocks must arrive as one patch");
    };
    let mut appends = BTreeMap::new();
    for change in changes {
        let ViewChange::UpdateItem {
            item_id, fields, ..
        } = change
        else {
            panic!("expected a batched item update, got {change:?}");
        };
        for delta in fields {
            match &delta.change {
                FieldChange::Append(text) => {
                    appends.insert(item_id.clone(), text.clone());
                }
                other => panic!("expected an append for {item_id}, got {other:?}"),
            }
        }
    }
    assert_eq!(appends.get("item-1").map(String::as_str), Some("-A"));
    assert_eq!(appends.get("item-2").map(String::as_str), Some("-B"));
    let snapshot = view.snapshot();
    assert_eq!(body_text(&snapshot.items[0]), "first-A");
    assert_eq!(body_text(&snapshot.items[1]), "second-B");
}

#[tokio::test]
async fn one_item_streams_its_part_fields_independently() {
    let session = Session::new(Stored::default());
    let mut reasoning = item(1, 1, "", false);
    reasoning.fields = BTreeMap::from([
        (
            ChatField::Part(PresentationPart::ReasoningText(0)),
            ContentBlock::from_text("thinking"),
        ),
        (
            ChatField::Part(PresentationPart::OutputText(0)),
            ContentBlock::from_text(""),
        ),
    ]);
    session.publish(reasoning.clone()).unwrap();
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    let (initial, mut updates) = view.subscribe();
    let mut client = Client::new(&initial);
    // The reasoning part is finished; only the answer part keeps streaming into the same item.
    let mut answer = reasoning;
    answer.revision = 2;
    answer.fields.insert(
        ChatField::Part(PresentationPart::OutputText(0)),
        ContentBlock::append(&ContentBlock::empty(), "answer"),
    );
    session.publish(answer).unwrap();
    let update = apply_next(&mut client, &mut updates, &view).await;
    let (_, expected, revision, _, fields) = expect_update(&update);
    assert_eq!(expected, 1);
    assert_eq!(revision, 2);
    assert!(matches!(
        fields,
        [
            FieldUpdate {
                field: ChatField::Part(PresentationPart::OutputText(0)),
                change: FieldChange::Append(text),
            },
            FieldUpdate {
                field: ChatField::Part(PresentationPart::ReasoningText(0)),
                change: FieldChange::Unchanged,
            },
        ] if text == "answer"
    ));
    let snapshot = view.snapshot();
    assert_eq!(
        snapshot.items[0]
            .part(PresentationPart::ReasoningText(0))
            .map(|block| block.text()),
        Some("thinking".to_owned())
    );
    assert_eq!(
        snapshot.items[0]
            .part(PresentationPart::OutputText(0))
            .map(|block| block.text()),
        Some("answer".to_owned())
    );
}

#[tokio::test]
async fn one_publish_that_changes_two_fields_commits_one_revision() {
    let session = Session::new(Stored::default());
    let arguments = ChatField::host("tool.arguments");
    let result = ChatField::host("tool.result");
    let mut first = item(1, 1, "", false);
    first.fields = BTreeMap::from([
        (arguments.clone(), ContentBlock::from_text("{\"q\":")),
        (result.clone(), ContentBlock::from_text("")),
    ]);
    session.publish(first.clone()).unwrap();
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    let (initial, mut updates) = view.subscribe();
    let mut client = Client::new(&initial);
    // One publish advances both content domains of the same identity. The batch carries a single
    // baseline and a single version commit, so a sequential per-change reducer cannot fail on the
    // second field.
    let mut second = first;
    second.revision = 2;
    let extended_arguments = ContentBlock::append(&second.fields[&arguments], "1}");
    second.fields.insert(arguments.clone(), extended_arguments);
    let extended_result = ContentBlock::append(&second.fields[&result], "ok");
    second.fields.insert(result.clone(), extended_result);
    session.publish(second).unwrap();
    let update = apply_next(&mut client, &mut updates, &view).await;
    let (_, expected, revision, _, fields) = expect_update(&update);
    assert_eq!(expected, 1);
    assert_eq!(revision, 2);
    assert_eq!(fields.len(), 2);
    assert!(matches!(
        fields,
        [
            FieldUpdate {
                field,
                change: FieldChange::Append(_),
            },
            FieldUpdate {
                field: other,
                change: FieldChange::Append(_),
            },
        ] if field != other
    ));
    let snapshot = view.snapshot();
    assert_eq!(
        snapshot.items[0]
            .field(&arguments)
            .map(|block| block.text()),
        Some("{\"q\":1}".to_owned())
    );
    assert_eq!(
        snapshot.items[0].field(&result).map(|block| block.text()),
        Some("ok".to_owned())
    );
}

#[tokio::test]
async fn host_field_keys_are_distinct_content_domains() {
    let session = Session::new(Stored::default());
    let arguments = ChatField::host("tool.arguments");
    let result = ChatField::host("tool.result");
    let mut call = item(1, 1, "", false);
    call.fields = BTreeMap::from([
        (arguments.clone(), ContentBlock::from_text("{\"path\":")),
        (result.clone(), ContentBlock::from_text("")),
    ]);
    session.publish(call.clone()).unwrap();
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    let (initial, mut updates) = view.subscribe();
    let mut client = Client::new(&initial);
    // Only the arguments domain streams; the result domain stays untouched, proving the two domains
    // are separate identities rather than one field the host must disambiguate from its text.
    let mut streamed = call;
    streamed.revision = 2;
    let extended = ContentBlock::append(&streamed.fields[&arguments], "\"a\"}");
    streamed.fields.insert(arguments.clone(), extended);
    session.publish(streamed).unwrap();
    let update = apply_next(&mut client, &mut updates, &view).await;
    let (_, _, _, _, fields) = expect_update(&update);
    assert!(matches!(
        fields,
        [
            FieldUpdate {
                field,
                change: FieldChange::Append(text),
            },
            FieldUpdate {
                change: FieldChange::Unchanged,
                ..
            },
        ] if field == &arguments && text == "\"a\"}"
    ));
    let snapshot = view.snapshot();
    assert_eq!(
        snapshot.items[0]
            .field(&arguments)
            .map(|block| block.text()),
        Some("{\"path\":\"a\"}".to_owned())
    );
    assert_eq!(
        snapshot.items[0].field(&result).map(|block| block.text()),
        Some(String::new())
    );
}

#[tokio::test]
async fn version_only_change_still_advances_the_consumer_revision() {
    let session = Session::new(Stored::default());
    let first = item(1, 1, "stable", false);
    session.publish(first.clone()).unwrap();
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    let (initial, mut updates) = view.subscribe();
    let mut client = Client::new(&initial);
    // The same shared block re-published under a newer revision must still reach the consumer, or its
    // local revision would lag and the next expected revision would never match.
    session.publish(appended(&first, 2, "")).unwrap();
    let update = apply_next(&mut client, &mut updates, &view).await;
    let (_, expected, revision, _, fields) = expect_update(&update);
    assert_eq!(expected, 1);
    assert_eq!(revision, 2);
    assert!(matches!(
        fields,
        [FieldUpdate {
            change: FieldChange::Unchanged,
            ..
        }]
    ));
}

#[tokio::test]
async fn removed_field_is_delivered_as_a_removal() {
    let session = Session::new(Stored::default());
    let result = ChatField::host("tool.result");
    let mut first = item(1, 1, "body", false);
    first
        .fields
        .insert(result.clone(), ContentBlock::from_text("partial"));
    session.publish(first.clone()).unwrap();
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    let (initial, mut updates) = view.subscribe();
    let mut client = Client::new(&initial);
    // Dropping a field is a real change, not a silently absent delta.
    let mut second = first;
    second.revision = 2;
    second.fields.remove(&result);
    session.publish(second).unwrap();
    let update = apply_next(&mut client, &mut updates, &view).await;
    let (_, _, _, _, fields) = expect_update(&update);
    assert!(matches!(
        fields,
        [
            FieldUpdate {
                field: ChatField::Body,
                change: FieldChange::Unchanged,
            },
            FieldUpdate {
                field,
                change: FieldChange::Remove,
            },
        ] if field == &result
    ));
    assert!(
        view.snapshot().items[0].field(&result).is_none(),
        "the removed field must not survive on the window"
    );
}

#[tokio::test]
async fn terminal_item_rejects_late_preview_updates() {
    let session = Session::new(Stored::default());
    session.publish(item(1, 5, "final", true)).unwrap();
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    // A preview that arrives after the terminal revision must not overwrite the accepted body.
    assert!(matches!(
        session.publish_preview(item(1, 5, "late", false)),
        Err(ChatError::Conflict(_))
    ));
    session.publish_preview(item(1, 4, "older", false)).unwrap();
    let snapshot = view.snapshot();
    assert_eq!(snapshot.items.len(), 1);
    assert_eq!(body_text(&snapshot.items[0]), "final");
    assert!(snapshot.items[0].is_terminal());
}

#[tokio::test]
async fn same_revision_cannot_carry_a_different_body() {
    let session = Session::new(Stored::default());
    session.publish(item(1, 4, "accepted", false)).unwrap();
    // Reusing a revision for different content would make the version watermark meaningless.
    assert!(matches!(
        session.publish(item(1, 4, "different", false)),
        Err(ChatError::Conflict(_))
    ));
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    assert_eq!(body_text(&view.snapshot().items[0]), "accepted");
}

#[tokio::test]
async fn terminal_identity_rejects_a_late_preview_after_cache_eviction() {
    let session = Session::new(Stored::default());
    // Durable history, so the identity is not retained as an unsaved reliable owner and the recent
    // cache alone does not keep its body.
    session.publish(durable(item(1, 5, "final", true))).unwrap();
    // Push the terminal identity out of the recent cache so memory no longer holds its body.
    for order in 2..=400 {
        session.publish(item(order, 1, "tail", false)).unwrap();
    }
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    assert!(
        view.snapshot()
            .items
            .iter()
            .all(|item| item.item_id != "item-1")
    );
    // A late preview for the evicted terminal identity is rejected instead of resurrecting a body.
    assert!(matches!(
        session.publish_preview(item(1, 5, "late", false)),
        Err(ChatError::Conflict(_))
    ));
    assert!(matches!(
        session.publish_preview(item(1, 6, "later", false)),
        Err(ChatError::Conflict(_))
    ));
    // The stale body never reappears in the window.
    assert!(
        view.snapshot()
            .items
            .iter()
            .all(|item| item.item_id != "item-1")
    );
}

#[tokio::test]
async fn late_update_after_the_terminal_marker_is_evicted_still_does_not_revive() {
    let session = Session::new(Stored::default());
    // A durable terminal identity, then more durable terminals than the bounded marker keeps, so the
    // marker for item-1 is evicted and only the order watermark still protects its slot.
    session.publish(durable(item(1, 1, "final", true))).unwrap();
    for order in 2..=4200 {
        session
            .publish(durable(item(order, 1, "tail", true)))
            .unwrap();
    }
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    assert!(
        view.snapshot()
            .items
            .iter()
            .all(|item| item.item_id != "item-1")
    );
    // Memory no longer holds item-1's marker, but an order slot is allocated strictly increasing and
    // never reused for another identity, so a publication at or below the evicted floor can only be a
    // late update for the committed identity and is rejected instead of resurrecting a stale body.
    assert!(matches!(
        session.publish_preview(item(1, 1, "late", false)),
        Err(ChatError::Conflict(_))
    ));
    assert!(matches!(
        session.publish_preview(item(1, 2, "later", false)),
        Err(ChatError::Conflict(_))
    ));
    assert!(
        view.snapshot()
            .items
            .iter()
            .all(|item| item.item_id != "item-1")
    );
}

#[tokio::test]
async fn first_byte_and_terminal_frames_are_immediate_not_coalesced() {
    let session = Session::new(Stored::default());
    session.publish(item(1, 1, "first", false)).unwrap();
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    let (_initial, mut updates) = view.subscribe();

    // A brand-new identity's first byte is a boundary. `now_or_never` polls the receiver once with a
    // no-op waker, so a frame that waited for the coalescing window would report itself pending here.
    session.publish(item(2, 1, "second", false)).unwrap();
    let Some(update) = futures::FutureExt::now_or_never(updates.next()).flatten() else {
        panic!("a first byte must be delivered without waiting for the coalescing window");
    };
    assert_eq!(update.priority(), ChatUpdatePriority::Immediate);

    // An execution terminal admission is a boundary too and must preempt any coalescing wait.
    session.publish(item(2, 2, "second done", true)).unwrap();
    let Some(update) = futures::FutureExt::now_or_never(updates.next()).flatten() else {
        panic!("a terminal frame must be delivered without waiting for the coalescing window");
    };
    assert_eq!(update.priority(), ChatUpdatePriority::Immediate);

    // A save acknowledgement is a boundary as well, not ordinary already-started text.
    session.confirm_saved("item-2", 2);
    let Some(update) = futures::FutureExt::now_or_never(updates.next()).flatten() else {
        panic!(
            "a save acknowledgement must be delivered without waiting for the coalescing window"
        );
    };
    assert_eq!(update.priority(), ChatUpdatePriority::Immediate);
}

#[tokio::test]
async fn watch_wait_is_cancel_safe_and_level_triggered() {
    let session = Session::new(Stored::default());
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    let watch = view.watch();
    let start = watch.current();
    assert_eq!(start, view.version());

    // Cancelled before the change: dropping a pending wait consumes nothing.
    let mut pending = Box::pin(watch.changed_since(start));
    assert!(futures::FutureExt::now_or_never(pending.as_mut()).is_none());
    drop(pending);
    session.publish(item(1, 1, "a", false)).unwrap();
    let after = view.version();
    assert_ne!(after, start);
    // Level-triggered: a change that happened while nothing waited is still observed.
    assert_eq!(
        futures::FutureExt::now_or_never(watch.changed_since(start)),
        Some(Some(after))
    );

    // Cancelled after the change arrived but before the wait completed: still not swallowed.
    let mut pending = Box::pin(watch.changed_since(after));
    assert!(futures::FutureExt::now_or_never(pending.as_mut()).is_none());
    session.publish(item(1, 2, "ab", false)).unwrap();
    let newer = view.version();
    drop(pending);
    assert_eq!(
        futures::FutureExt::now_or_never(watch.changed_since(after)),
        Some(Some(newer))
    );
    // The level check is repeatable; observing a version never advances consumer state.
    assert_eq!(
        futures::FutureExt::now_or_never(watch.changed_since(after)),
        Some(Some(newer))
    );
}

#[tokio::test]
async fn take_is_synchronous_and_only_advances_on_a_real_change() {
    let session = Session::new(Stored::default());
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    let (initial, mut updates) = view.subscribe();
    assert_eq!(initial.version, view.version());

    // Nothing changed since the subscription: no empty frame and no baseline move.
    assert!(updates.take().is_none());
    assert_eq!(view.version(), initial.version);

    // A real change is taken as a patch from the delivered baseline to the version just read.
    session.publish(item(1, 1, "ready", false)).unwrap();
    let update = updates.take().expect("a pending change");
    let ChatUpdate::Patch { from, to, .. } = &update else {
        panic!("a change must be delivered as a patch");
    };
    assert_eq!(*from, initial.version);
    assert_eq!(*to, view.version());
    // The baseline advanced atomically, so the same version is never delivered twice.
    assert!(updates.take().is_none());
}

#[tokio::test]
async fn two_subscriptions_keep_their_own_baseline() {
    let session = Session::new(Stored::default());
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    let (first_baseline, mut first) = view.subscribe();
    let (second_baseline, mut second) = view.subscribe();

    session.publish(item(1, 1, "one", false)).unwrap();
    let first_update = first.take().expect("first consumer change");
    let ChatUpdate::Patch {
        from: first_from,
        to: first_to,
        ..
    } = &first_update
    else {
        panic!("a change must be delivered as a patch");
    };
    assert_eq!(*first_from, first_baseline.version);

    // The second consumer never advanced, so it still diffs against its own baseline.
    session.publish(item(2, 1, "two", false)).unwrap();
    let second_update = second.take().expect("second consumer change");
    let ChatUpdate::Patch {
        from: second_from,
        to: second_to,
        changes,
        ..
    } = &second_update
    else {
        panic!("a change must be delivered as a patch");
    };
    assert_eq!(*second_from, second_baseline.version);
    assert_eq!(*second_to, view.version());
    assert!(
        !changes.is_empty(),
        "the second consumer saw the whole diff"
    );

    // The first consumer continues from where it advanced, not from the second consumer's baseline.
    let first_next = first.take().expect("first consumer next change");
    let ChatUpdate::Patch {
        from: next_from, ..
    } = &first_next
    else {
        panic!("a change must be delivered as a patch");
    };
    assert_eq!(*next_from, *first_to);
}

#[tokio::test]
async fn a_pending_wait_does_not_block_focus_or_load() {
    let session = Session::new(Stored {
        items: (1..=40)
            .map(|order| (order, item(order, 1, "stored", true)))
            .collect(),
    });
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    let (_initial, mut updates) = view.subscribe();

    // `next` parks on a version change without holding any lock; a concurrent focus must still
    // complete, and its own version change is what wakes the wait.
    let waited = updates.next();
    let focused = view.focus(ChatFocus::Around("item-40".into()));
    let (update, focused) = tokio::join!(waited, focused);
    assert!(
        focused.is_ok(),
        "focus must proceed while a wait is pending"
    );
    assert!(
        matches!(update, Some(ChatUpdate::Reset(_))),
        "the focus change wakes the pending wait as a reset"
    );

    // The same holds for a history page load: the wait must not hold a lock that blocks it.
    let (_again, mut updates) = view.subscribe();
    let waited = updates.next();
    let loaded = view.load(Direction::Older);
    let (update, loaded) = tokio::join!(waited, loaded);
    let loaded = loaded.expect("load must proceed while a wait is pending");
    assert!(
        !loaded.items.is_empty(),
        "the load produced a readable window"
    );
    assert!(
        update.is_some(),
        "the load's version change wakes the pending wait"
    );
}

#[tokio::test]
async fn ordinary_text_appends_merge_into_one_frame() {
    let session = Session::new(Stored::default());
    let first = item(1, 1, "start", false);
    session.publish(first.clone()).unwrap();
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    let (initial, mut updates) = view.subscribe();
    let mut client = Client::new(&initial);

    // Three appends land inside one coalescing window while the consumer is already waiting.
    let second = appended(&first, 2, "-a");
    let third = appended(&second, 3, "-b");
    let fourth = appended(&third, 4, "-c");
    let waited = updates.next();
    let publish = async move {
        session.publish(second).unwrap();
        session.publish(third).unwrap();
        session.publish(fourth).unwrap();
    };
    let (update, ()) = tokio::join!(waited, publish);
    let update = update.expect("one coalesced frame");
    assert_eq!(update.priority(), ChatUpdatePriority::Coalesced);
    let (item_id, expected, revision, _, fields) = expect_update(&update);
    assert_eq!(item_id, "item-1");
    assert_eq!(expected, 1);
    assert_eq!(revision, 4);
    assert!(matches!(
        fields,
        [FieldUpdate {
            field: ChatField::Body,
            change: FieldChange::Append(text),
        }] if text == "-a-b-c"
    ));
    client.apply(&update);
    client.assert_matches(&view.snapshot());
}

#[tokio::test]
async fn a_terminal_boundary_preempts_the_text_coalescing_window() {
    let session = Session::new(Stored::default());
    let first = item(1, 1, "hello", false);
    session.publish(first.clone()).unwrap();
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    let (initial, mut updates) = view.subscribe();
    let mut client = Client::new(&initial);

    // An ordinary append starts a coalescing wait.
    session.publish(appended(&first, 2, " world")).unwrap();
    let mut pending = Box::pin(updates.next());
    assert!(
        futures::FutureExt::now_or_never(pending.as_mut()).is_none(),
        "ordinary text holds the coalescing window"
    );

    // A terminal admission during the window must be delivered at once instead of waiting it out.
    session.publish(item(1, 3, "hello world!", true)).unwrap();
    let update = futures::FutureExt::now_or_never(pending.as_mut())
        .flatten()
        .expect("the terminal boundary must preempt the coalescing window");
    assert_eq!(update.priority(), ChatUpdatePriority::Immediate);
    client.apply(&update);
    client.assert_matches(&view.snapshot());
    assert!(view.snapshot().items[0].is_terminal());
}

#[tokio::test]
async fn same_revision_preview_to_complete_updates_the_omitted_status() {
    let full = "f".repeat(300 * 1024);
    let session = Session::new(Stored {
        items: [(1, item(1, 1, &full, true))].into(),
    });
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    assert!(view.snapshot().items[0].omitted_bytes > 0);
    let (initial, mut updates) = view.subscribe();
    let mut client = Client::new(&initial);
    // The host asks for the complete body; the window now holds the whole body at the same revision.
    let complete = view.read_complete("item-1").await.unwrap().unwrap();
    assert_eq!(complete.omitted_bytes, 0);
    // Re-publishing the same identity at the same revision keeps the item set unchanged, so the frame
    // diffs the complete copy against the bounded baseline and must carry the new (zero) omitted
    // status together with the field content in one commit.
    session.publish(item(1, 1, &full, true)).unwrap();
    let update = apply_next(&mut client, &mut updates, &view).await;
    let (item_id, expected, revision, omitted, _) = expect_update(&update);
    assert_eq!(item_id, "item-1");
    assert_eq!(expected, 1);
    assert_eq!(revision, 1);
    assert_eq!(omitted, 0);
    assert_eq!(body_text(&view.snapshot().items[0]), full);
}

/// Reading the complete body of a still-unsaved identity must not roll the save watermark back.
///
/// The window's complete-content cache stores a body snapshot taken when the body was requested; the
/// `saved` watermark and execution lifecycle are independent timeline-owned facts. A save
/// acknowledgement that lands at the same revision after the read must survive the next snapshot
/// (and the real reducer frame) instead of being restored to the cached `false`.
#[tokio::test]
async fn read_complete_then_save_keeps_the_new_save_watermark() {
    let session = Session::new(Stored::default());
    // A reliable, still-streaming item: the window serves its complete in-flight body.
    session.publish(item(1, 1, "hello", false)).unwrap();
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    assert_eq!(view.snapshot().items[0].omitted_bytes, 0);
    // The host asks for the complete body while the item is still unsaved, filling the cache with a
    // snapshot that does not yet carry the save watermark.
    let cached = view.read_complete("item-1").await.unwrap().unwrap();
    assert!(!cached.is_saved());

    let (initial, mut updates) = view.subscribe();
    let mut client = Client::new(&initial);
    // The writer acknowledges exactly this revision. This flips `saved` only; the cached body must
    // not let the overlay roll it back.
    session.confirm_saved("item-1", 1);
    let update = apply_next(&mut client, &mut updates, &view).await;
    let (item_id, expected, revision, omitted, _) = expect_update(&update);
    assert_eq!(item_id, "item-1");
    assert_eq!(expected, 1);
    assert_eq!(revision, 1);
    assert_eq!(omitted, 0);
    let snapshot = view.snapshot();
    assert!(
        snapshot.items[0].is_saved(),
        "a cached body must never undo a save acknowledgement"
    );
    assert!(snapshot.items[0].is_streaming());
    // The body stays the complete in-flight body; the retained cache did not regress it to a preview.
    assert_eq!(snapshot.items[0].omitted_bytes, 0);
    assert_eq!(body_text(&snapshot.items[0]), "hello");
}

/// Reading the complete body of a streaming identity must not roll a terminal admission back.
///
/// A terminal admission can arrive at the same content revision as the read (execution finished
/// without a content change). The execution lifecycle is an independent timeline-owned fact, so the
/// cached streaming body must not restore `Streaming` over the window's `Terminal`.
#[tokio::test]
async fn read_complete_then_terminal_keeps_the_terminal_lifecycle() {
    let session = Session::new(Stored::default());
    session
        .publish_preview(item(1, 1, "reasoning done", false))
        .unwrap();
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    assert_eq!(view.snapshot().items[0].omitted_bytes, 0);
    // The cached body snapshot still claims `Streaming`.
    let cached = view.read_complete("item-1").await.unwrap().unwrap();
    assert!(cached.is_streaming());

    let (initial, mut updates) = view.subscribe();
    let mut client = Client::new(&initial);
    // The producer admits the terminal fact at the same revision; the cached body must not roll it
    // back to `Streaming` on the next snapshot or reducer frame.
    session.publish(item(1, 1, "reasoning done", true)).unwrap();
    let update = apply_next(&mut client, &mut updates, &view).await;
    assert_eq!(update.priority(), ChatUpdatePriority::Immediate);
    let snapshot = view.snapshot();
    assert!(
        snapshot.items[0].is_terminal(),
        "a cached body must never undo a terminal admission"
    );
    assert_eq!(body_text(&snapshot.items[0]), "reasoning done");
}

#[tokio::test]
async fn save_confirmation_keeps_identity_order_and_body() {
    let session = Session::new(Stored::default());
    session.publish(item(1, 3, "streamed", false)).unwrap();
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    let before = view.snapshot().items[0].clone();
    session.confirm_saved("item-1", 3);
    let after = view.snapshot().items[0].clone();
    // The save watermark is its own fact: acknowledgement must not advance the execution lifecycle to
    // terminal, nor change identity, order, revision or body.
    assert!(after.is_saved());
    assert!(after.is_streaming());
    assert_eq!(after.item_id, before.item_id);
    assert_eq!(after.order, before.order);
    assert_eq!(after.revision, before.revision);
    assert_eq!(body_text(&after), body_text(&before));

    // Acknowledging a streaming identity never freezes it: a later revision still advances and is
    // again unsaved until the writer acknowledges the new revision.
    let mut next = appended(&after, 4, " more");
    next.saved = false;
    session.publish(next).unwrap();
    let advanced = view.snapshot().items[0].clone();
    assert_eq!(advanced.revision, 4);
    assert_eq!(body_text(&advanced), "streamed more");
    assert!(advanced.is_streaming());
    assert!(!advanced.is_saved());
}

#[tokio::test]
async fn terminal_item_can_stay_unsaved_and_keeps_its_writer_ownership() {
    let session = Session::new(Stored::default());
    // A terminal item is accepted without any save acknowledgement: execution finished before the
    // writer persisted. The reliable uncommitted owner is still retained for the writer to retry.
    session.publish(item(1, 4, "final", true)).unwrap();
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    let visible = view.snapshot().items[0].clone();
    assert!(visible.is_terminal());
    assert!(!visible.is_saved());
    // The terminal acknowledgement is independent of writer speed: a late preview is refused from the
    // execution lifecycle alone, before any save confirmation.
    assert!(matches!(
        session.publish_preview(item(1, 4, "stale", false)),
        Err(ChatError::Conflict(_))
    ));
    assert_eq!(body_text(&view.snapshot().items[0]), "final");
    // The retained effect is not a speculative preview, so a preview sweep does not release it.
    session.drop_previews_with_prefix("item-");
    assert!(
        view.snapshot()
            .items
            .iter()
            .any(|item| item.item_id == "item-1")
    );

    // Acknowledging exactly this revision flips the save watermark only and keeps the terminal
    // identity, order, revision and body.
    session.confirm_saved("item-1", 4);
    let saved = view.snapshot().items[0].clone();
    assert!(saved.is_saved());
    assert!(saved.is_terminal());
    assert_eq!(saved.item_id, visible.item_id);
    assert_eq!(saved.order, visible.order);
    assert_eq!(saved.revision, visible.revision);
    assert_eq!(body_text(&saved), "final");
}

#[tokio::test]
async fn stale_writer_ack_does_not_mark_a_newer_revision_saved() {
    let session = Session::new(Stored::default());
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    let first = item(1, 1, "one", false);
    session.publish(first.clone()).unwrap();
    session.confirm_saved("item-1", 1);
    assert!(view.snapshot().items[0].is_saved());

    // The content advances past the acknowledged revision and is unsaved again.
    session.publish(appended(&first, 2, " two")).unwrap();
    assert_eq!(view.snapshot().items[0].revision, 2);
    assert!(!view.snapshot().items[0].is_saved());

    // An out-of-order acknowledgement of the older revision must not mark the newer one durable.
    session.confirm_saved("item-1", 1);
    assert_eq!(view.snapshot().items[0].revision, 2);
    assert!(!view.snapshot().items[0].is_saved());

    // Only the exact acknowledgement advances the save watermark.
    session.confirm_saved("item-1", 2);
    assert!(view.snapshot().items[0].is_saved());
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
    assert_eq!(body_text(&visible.items[0]), "completed");
    assert!(visible.items[0].is_terminal());
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
    assert_eq!(body_text(&visible.items[0]), "canonical");
    assert!(visible.items[0].is_terminal());
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
    // The window's older boundary only becomes true here, which a patch cannot express; the slow
    // subscriber still gets one coherent frame instead of a spliced hole.
    assert!(matches!(updates.next().await, Some(ChatUpdate::Reset(_))));
    let old = old.snapshot();
    assert_eq!(old.items.len(), 1);
    assert_eq!(old.items[0].order, 1);
    assert!(old.has_newer);
    assert_eq!(latest.snapshot().items.last().unwrap().order, 200);
}

#[tokio::test]
async fn coalesced_window_versions_arrive_as_one_patch() {
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
    // Many committed versions were coalesced into one frame, and the focus and older boundary did
    // not move, so the delivery stays a continuous patch from the subscriber's own baseline instead
    // of degrading to a reset.
    let ChatUpdate::Patch {
        from, to, changes, ..
    } = update
    else {
        panic!("a coalesced tail update must be delivered as a patch");
    };
    assert!(to > from);
    assert!(matches!(&changes[..], [ViewChange::Splice { items, .. }]
            if items.first().unwrap().order == 269 && items.last().unwrap().order == 300));
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
async fn newer_revision_keeps_save_responsibility_and_emits_typed_append() {
    let session = Session::new(Stored::default());
    let first = item(1, 10, "hello", false);
    session.publish(first.clone()).unwrap();
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    let (initial, mut updates) = view.subscribe();
    let mut client = Client::new(&initial);
    session.publish(appended(&first, 11, " world")).unwrap();
    let update = apply_next(&mut client, &mut updates, &view).await;
    let (item_id, expected, revision, _, fields) = expect_update(&update);
    assert_eq!(item_id, "item-1");
    assert_eq!(expected, 10);
    assert_eq!(revision, 11);
    assert!(matches!(
        fields,
        [FieldUpdate {
            field: ChatField::Body,
            change: FieldChange::Append(text),
        }] if text == " world"
    ));
    session.confirm_saved("item-1", 10);
    assert!(view.snapshot().items[0].is_streaming());
    session.confirm_saved("item-1", 11);
    assert!(view.snapshot().items[0].is_saved());
    assert!(view.snapshot().items[0].is_streaming());
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
    assert!(snapshot.items[0].is_streaming());
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
    assert_eq!(body_text(&snapshot.items[0]), "first");
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
    assert_eq!(body_text(&first), "complete");
    assert!(first.is_streaming());
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
    assert_eq!(body_text(&snapshot.items[0]), "new");
    assert!(snapshot.items[0].is_streaming());
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

#[tokio::test]
async fn readiness_wait_is_cancel_safe_and_wakes_at_once_on_a_boundary() {
    let session = Session::new(Stored::default());
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    let watch = view.watch();
    let start = watch.current();
    assert_eq!(start, view.version());

    // Cancelled before any change: dropping the pending readiness wait consumes nothing.
    let mut pending = Box::pin(watch.wait(start));
    assert!(futures::FutureExt::now_or_never(pending.as_mut()).is_none());
    drop(pending);

    // A boundary that already happened before the wait is observed level-triggered and delivered at
    // once instead of after a coalescing window: a first byte must never be delayed.
    session.publish(item(1, 1, "first", false)).unwrap();
    let after = view.version();
    assert_ne!(after, start);
    assert_eq!(
        futures::FutureExt::now_or_never(watch.wait(start)),
        Some(Some(after))
    );

    // A later boundary is likewise ready immediately for a consumer that only delivered `after`.
    let second = item(2, 1, "second", false);
    session.publish(second.clone()).unwrap();
    let newer = view.version();
    assert_eq!(
        futures::FutureExt::now_or_never(watch.wait(after)),
        Some(Some(newer))
    );

    // Ordinary already-started text is not a boundary: it holds one coalescing window instead of
    // being delivered the instant it lands. The delta must extend the *resident* block so it is a
    // genuine append, not a replacement.
    session.publish_preview(appended(&second, 2, "-x")).unwrap();
    assert!(
        futures::FutureExt::now_or_never(watch.wait(newer)).is_none(),
        "ordinary text must wait for the fixed coalescing window, not be delivered per token"
    );
}

#[tokio::test(start_paused = true)]
async fn coalescing_deadline_is_fixed_and_not_restarted_by_a_later_delta() {
    let session = Session::new(Stored::default());
    let first = item(1, 1, "start", false);
    session.publish(first.clone()).unwrap();
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    let (_initial, mut updates) = view.subscribe();

    // The first ordinary append anchors the single coalescing window at "now".
    let second = appended(&first, 2, "-a");
    session.publish(second.clone()).unwrap();
    let mut pending = Box::pin(updates.next());
    assert!(
        futures::FutureExt::now_or_never(pending.as_mut()).is_none(),
        "an ordinary append holds the coalescing window"
    );

    // A later delta arrives before the deadline. It advances content but must not restart the window.
    tokio::time::advance(std::time::Duration::from_millis(20)).await;
    session.publish(appended(&second, 3, "-b")).unwrap();
    assert!(
        futures::FutureExt::now_or_never(pending.as_mut()).is_none(),
        "a later delta must not restart the coalescing deadline"
    );

    // Past the *original* deadline the merged frame is ready. If the later delta had restarted the
    // window the wait would still be pending here, so this kills that regression.
    tokio::time::advance(std::time::Duration::from_millis(20)).await;
    let update = futures::FutureExt::now_or_never(pending.as_mut())
        .flatten()
        .expect("the frame is delivered at the deadline anchored by the first delta");
    assert_eq!(update.priority(), ChatUpdatePriority::Coalesced);
    let (item_id, expected, revision, _, fields) = expect_update(&update);
    assert_eq!(item_id, "item-1");
    assert_eq!(expected, 1);
    assert_eq!(revision, 3);
    assert!(matches!(
        fields,
        [FieldUpdate {
            field: ChatField::Body,
            change: FieldChange::Append(text),
        }] if text == "-a-b"
    ));
}

#[tokio::test]
async fn lock_outside_wait_and_synchronous_take_merge_ordinary_text_once() {
    // The runtime/bridge shape: wait for readiness on a cloned watch outside the window, then take
    // the frame in one short synchronous step. It must produce the same single merged frame as
    // `next`, proving HTTP and FRB share one coalescing rule.
    let session = Session::new(Stored::default());
    let first = item(1, 1, "start", false);
    session.publish(first.clone()).unwrap();
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    let (initial, mut updates) = view.subscribe();
    let mut client = Client::new(&initial);
    let watch = view.watch();

    let second = appended(&first, 2, "-a");
    let third = appended(&second, 3, "-b");
    let fourth = appended(&third, 4, "-c");
    let waited = async {
        // The wait consumes the *observed* wake watermark, never the delivered baseline.
        let seen = updates.observed_version();
        watch.wait(seen).await
    };
    let publish = async move {
        session.publish(second).unwrap();
        session.publish(third).unwrap();
        session.publish(fourth).unwrap();
    };
    let (ready, ()) = tokio::join!(waited, publish);
    assert!(ready.is_some(), "readiness is observed");

    // Exactly one diff is computed, in the short synchronous take.
    let update = updates.take().expect("one merged frame");
    assert_eq!(update.priority(), ChatUpdatePriority::Coalesced);
    let (item_id, expected, revision, _, fields) = expect_update(&update);
    assert_eq!(item_id, "item-1");
    assert_eq!(expected, 1);
    assert_eq!(revision, 4);
    assert!(matches!(
        fields,
        [FieldUpdate {
            field: ChatField::Body,
            change: FieldChange::Append(text),
        }] if text == "-a-b-c"
    ));
    client.apply(&update);
    client.assert_matches(&view.snapshot());
    // The baseline advanced atomically with the single diff, so the same version is never retaken.
    assert!(updates.take().is_none());
}

#[tokio::test]
async fn history_window_skips_unrelated_live_updates_and_keeps_patch_continuity() {
    let big = "h".repeat(300 * 1024);
    let session = Session::new(Stored {
        items: (1..=60)
            .map(|order| {
                let text = if order == 44 {
                    big.clone()
                } else {
                    "stored".to_owned()
                };
                (order, item(order, 1, &text, true))
            })
            .collect(),
    });
    // Read a bounded window anchored on the newest stored identity, so the window currently has no
    // newer content (has_newer is false) and the first live message must flip it. `item-44` is large
    // enough to be shown as a bounded preview, so a later complete-body read is a visible change.
    let view = session
        .open_chat(ChatFocus::Around("item-60".into()))
        .await
        .unwrap();
    let (initial, mut updates) = view.subscribe();
    let mut client = Client::new(&initial);
    assert!(matches!(&initial.focus, ChatFocus::Around(_)));
    assert_eq!(updates.delivered_version(), initial.version);
    assert_eq!(updates.observed_version(), initial.version);
    let history_items: Vec<String> = initial
        .items
        .iter()
        .map(|item| item.item_id.clone())
        .collect();
    assert!(
        initial
            .items
            .iter()
            .find(|item| item.item_id == "item-44")
            .is_some_and(|item| item.omitted_bytes > 0),
        "the big history item starts as a bounded preview"
    );

    // A live message arrives. The history body does not move, so the only thing to show is the
    // new-content hint, delivered as a patch from the subscriber's own version.
    let live = item(61, 1, "live", false);
    session.publish(live.clone()).unwrap();
    let hint = updates.next().await.expect("the new-content hint");
    let ChatUpdate::Patch {
        from: hint_from,
        to: hint_to,
        changes,
        has_newer,
        ..
    } = &hint
    else {
        panic!("an unrelated live item must be a patch, not a reset: {hint:?}");
    };
    assert_eq!(*hint_from, initial.version);
    assert!(changes.is_empty(), "the history body must not be rebuilt");
    assert!(*has_newer, "the new-content hint must be raised");
    let last_delivered = *hint_to;
    client.apply(&hint);
    client.assert_matches(&view.snapshot());
    assert_eq!(updates.delivered_version(), last_delivered);

    // Several unrelated live updates are skipped: no body diff, no reset, no repeated empty patch.
    // They advance only the observed wake watermark, never the client's delivered baseline.
    let mut previous = live.clone();
    for revision in 2..=6 {
        let next = appended(&previous, revision, "-token");
        session.publish_preview(next.clone()).unwrap();
        previous = next;
    }
    assert!(
        updates.take().is_none(),
        "an unrelated live update must not emit a frame"
    );
    assert_eq!(
        updates.delivered_version(),
        last_delivered,
        "a skipped update must not move the delivered baseline"
    );
    assert!(
        updates.observed_version() > last_delivered,
        "a skipped update only advances the observed wake watermark"
    );
    // The waiting path blocks instead of repeatedly waking for the unrelated update.
    assert!(
        futures::FutureExt::now_or_never(updates.next()).is_none(),
        "no frame is deliverable for an unchanged history window"
    );
    // The visible history rows are untouched: identity and order are stable.
    assert_eq!(
        view.snapshot()
            .items
            .iter()
            .map(|item| item.item_id.clone())
            .collect::<Vec<_>>(),
        history_items
    );

    // A genuinely visible change inside the window: the host reads the complete body of a history
    // item. The delivered patch must continue from the last received version, not from a skipped one.
    view.read_complete("item-44").await.unwrap().unwrap();
    let update = updates.next().await.expect("the complete-body frame");
    let ChatUpdate::Patch {
        from, to, changes, ..
    } = &update
    else {
        panic!("the complete body must arrive as a patch, not a reset: {update:?}");
    };
    assert_eq!(
        *from, last_delivered,
        "the patch must continue from the last delivered version"
    );
    assert!(matches!(
        &changes[..],
        [ViewChange::UpdateItem {
            item_id,
            expected_revision,
            revision,
            omitted_bytes,
            ..
        }] if item_id == "item-44"
            && *expected_revision == 1
            && *revision == 1
            && *omitted_bytes == 0
    ));
    let delivered_to = *to;
    client.apply(&update);
    client.assert_matches(&view.snapshot());
    assert_eq!(updates.delivered_version(), delivered_to);
    // Only the requested body completed; the visible history rows kept their identity and order.
    assert_eq!(
        view.snapshot()
            .items
            .iter()
            .map(|item| item.item_id.clone())
            .collect::<Vec<_>>(),
        history_items
    );
    assert_eq!(
        body_text(
            view.snapshot()
                .items
                .iter()
                .find(|item| item.item_id == "item-44")
                .unwrap()
        ),
        big
    );

    // A user paging older must also continue from the last delivered version, as a patch rather than
    // a reset that would hide a broken watermark.
    let loaded = view.load(Direction::Older).await.unwrap();
    assert!(loaded.items.len() > history_items.len());
    let paged = updates.next().await.expect("the paging frame");
    let ChatUpdate::Patch { from, .. } = &paged else {
        panic!("paging must stay a patch from the delivered baseline: {paged:?}");
    };
    assert_eq!(*from, delivered_to);
    client.apply(&paged);
    client.assert_matches(&view.snapshot());

    // Returning to the live tail restores the authoritative content in one coherent frame.
    view.focus(ChatFocus::Latest).await.unwrap();
    let tail = updates.next().await.expect("the tail frame");
    client.apply(&tail);
    client.assert_matches(&view.snapshot());
    let snapshot = view.snapshot();
    assert!(matches!(&snapshot.focus, ChatFocus::Latest));
    let newest = snapshot.items.last().expect("the live tail is shown");
    assert_eq!(newest.item_id, "item-61");
    assert_eq!(body_text(newest), "live-token-token-token-token-token");
}

#[tokio::test]
async fn reading_a_complete_body_advances_the_window_version_and_notifies() {
    let full = "z".repeat(300 * 1024);
    let session = Session::new(Stored {
        items: [(1, item(1, 1, &full, true))].into(),
    });
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    let (initial, mut updates) = view.subscribe();
    let mut client = Client::new(&initial);
    // The window shows a bounded preview, so the subscriber's delivered window holds only that.
    assert!(initial.items[0].omitted_bytes > 0);
    let observed_before = updates.observed_version();

    // Asking for the complete body changes the window's *visible* body at the same item revision.
    let complete = view.read_complete("item-1").await.unwrap().unwrap();
    assert_eq!(complete.omitted_bytes, 0);
    assert!(
        view.version() > observed_before,
        "a visible complete-body read must advance the window version"
    );
    let update = updates.next().await.expect("the complete-body frame");
    let ChatUpdate::Patch {
        from, to, changes, ..
    } = &update
    else {
        panic!("the complete body must be a patch, not a reset: {update:?}");
    };
    assert_eq!(*from, initial.version);
    assert!(matches!(
        &changes[..],
        [ViewChange::UpdateItem {
            item_id,
            expected_revision,
            revision,
            omitted_bytes,
            ..
        }] if item_id == "item-1"
            && *expected_revision == 1
            && *revision == 1
            && *omitted_bytes == 0
    ));
    let delivered_to = *to;
    client.apply(&update);
    client.assert_matches(&view.snapshot());
    assert_eq!(body_text(&view.snapshot().items[0]), full);
    assert_eq!(updates.delivered_version(), delivered_to);

    // A second request for the now-retained body changes nothing and must not bump the version again.
    view.read_complete("item-1").await.unwrap().unwrap();
    assert_eq!(
        view.version(),
        delivered_to,
        "a retained complete body must not bump the version again"
    );
    assert!(updates.take().is_none());

    // A late read after the identity has left the window must not write back or bump the version.
    for order in 2..=300 {
        session.publish(item(order, 1, "tail", false)).unwrap();
    }
    assert!(
        view.snapshot()
            .items
            .iter()
            .all(|item| item.item_id != "item-1")
    );
    let version_before_late = view.version();
    view.read_complete("item-1").await.unwrap().unwrap();
    assert_eq!(
        view.version(),
        version_before_late,
        "a late read off the window must not bump the version"
    );
    assert!(
        view.snapshot()
            .items
            .iter()
            .all(|item| item.item_id != "item-1"),
        "a late read must not resurrect the identity"
    );
}

/// The writer's preview release may only drop identities this session never committed.
///
/// The history writer confirms the exact `(identity, revision)` it saved and then releases the
/// attempt's speculative previews with `drop_previews_with_prefix`. The committed presentation row is
/// no longer in the reliable-uncommitted map at that point, so it must still be protected by its own
/// terminal fact: otherwise the complete body and its window position would vanish from every view
/// even though durable history holds the exact identity.
#[tokio::test]
async fn committed_identity_survives_the_preview_release_after_a_save_ack() {
    let session = Session::new(Stored::default());
    let prefix = pl_core::chat::presentation_prefix("attempt");
    let id = pl_core::chat::presentation_item_id(
        "attempt",
        "message-1",
        Some(PresentationPart::OutputText(0)),
    );
    // The streamed prefix is speculative, so the window shows it without owning it.
    session
        .publish_preview(identified(&id, 1, 1, "realtime ", false))
        .unwrap();
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    assert_eq!(body_text(&view.snapshot().items[0]), "realtime ");

    // The terminal commit publishes the complete body on the same identity and the writer confirms
    // exactly that revision...
    session
        .publish(identified(&id, 1, 2, "realtime delayed first token", true))
        .unwrap();
    session.confirm_saved(&id, 2);
    assert!(session.is_committed(&id));
    // ...and only then releases the attempt's speculative previews.
    session.drop_previews_with_prefix(&prefix);

    let snapshot = view.snapshot();
    assert_eq!(
        snapshot.items.len(),
        1,
        "the committed row must stay in the window"
    );
    assert_eq!(snapshot.items[0].item_id, id);
    assert_eq!(
        snapshot.items[0].order, 1,
        "the committed position is final"
    );
    assert_eq!(snapshot.items[0].revision, 2);
    assert_eq!(
        body_text(&snapshot.items[0]),
        "realtime delayed first token"
    );
    assert!(snapshot.items[0].saved);
    assert!(snapshot.items[0].is_terminal());
}

/// The same release still drops an identity that never committed, so the protection is not a leak.
#[tokio::test]
async fn preview_release_still_removes_an_identity_that_never_committed() {
    let session = Session::new(Stored::default());
    let prefix = pl_core::chat::presentation_prefix("attempt");
    let dropped = pl_core::chat::presentation_item_id(
        "attempt",
        "message-2",
        Some(PresentationPart::OutputText(0)),
    );
    session.publish_preview(item(1, 1, "kept", false)).unwrap();
    session
        .publish_preview(identified(&dropped, 2, 1, "in-flight", false))
        .unwrap();
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    assert_eq!(view.snapshot().items.len(), 2);

    session.drop_previews_with_prefix(&prefix);

    let snapshot = view.snapshot();
    assert_eq!(snapshot.items.len(), 1);
    assert_eq!(snapshot.items[0].item_id, "item-1");
}

/// The release spares the identities the commit itself confirmed.
///
/// The writer knows exactly which identities its transaction saved, so it does not depend on the
/// session having observed the terminal publication: an effect the owner installed behind is folded
/// into a snapshot without a live publish, and its confirmed rows must survive the release all the
/// same. A sibling preview the commit did not confirm is still released.
#[tokio::test]
async fn the_preview_release_spares_the_identities_the_commit_just_confirmed() {
    let session = Session::new(Stored::default());
    let prefix = pl_core::chat::presentation_prefix("attempt");
    let confirmed = pl_core::chat::presentation_item_id(
        "attempt",
        "message-1",
        Some(PresentationPart::OutputText(0)),
    );
    let dropped = pl_core::chat::presentation_item_id(
        "attempt",
        "message-2",
        Some(PresentationPart::OutputText(0)),
    );
    session
        .publish_preview(identified(&confirmed, 1, 1, "realtime ", false))
        .unwrap();
    session
        .publish_preview(identified(&dropped, 2, 1, "in-flight", false))
        .unwrap();
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    assert_eq!(view.snapshot().items.len(), 2);

    session.drop_previews_with_prefix_except(&prefix, [confirmed.as_str()]);

    let snapshot = view.snapshot();
    assert_eq!(snapshot.items.len(), 1);
    assert_eq!(snapshot.items[0].item_id, confirmed);
    assert_eq!(snapshot.items[0].order, 1);
}

/// A slow observation of a committed identity is a stale frame, never newer content.
///
/// Once the terminal body is published, a preview for the same identity is rejected with the
/// session's typed conflict: it must neither replace the committed row nor re-open it for the writer's
/// later release to remove, so the window body and revision stay the committed ones.
#[tokio::test]
async fn a_late_preview_never_reopens_a_committed_terminal_identity() {
    let session = Session::new(Stored::default());
    let prefix = pl_core::chat::presentation_prefix("attempt");
    let id = pl_core::chat::presentation_item_id(
        "attempt",
        "message-1",
        Some(PresentationPart::OutputText(0)),
    );
    session
        .publish_preview(identified(&id, 1, 1, "realtime ", false))
        .unwrap();
    session
        .publish(identified(&id, 1, 5, "realtime delayed first token", true))
        .unwrap();
    session.confirm_saved(&id, 5);
    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    let version = view.version();

    assert!(matches!(
        session.publish_preview(identified(&id, 1, 6, "realtime", false)),
        Err(ChatError::Conflict(_))
    ));
    session.drop_previews_with_prefix(&prefix);

    assert_eq!(view.version(), version, "a rejected preview is not a frame");
    let snapshot = view.snapshot();
    assert_eq!(snapshot.items.len(), 1);
    assert_eq!(snapshot.items[0].revision, 5);
    assert_eq!(
        body_text(&snapshot.items[0]),
        "realtime delayed first token"
    );
    assert!(snapshot.items[0].is_terminal());
}

/// A save acknowledgement that precedes the last preview keeps the committed body and position.
///
/// This is the real ordering the writer produces: the acknowledgement, then the preview release, both
/// while the producer may still be publishing one more revision of the same identity. The late preview
/// is rejected as the typed conflict — it cannot become newer content for a terminal identity — and
/// neither it nor the release may erase the committed row.
#[tokio::test]
async fn a_save_ack_before_the_last_preview_never_releases_the_committed_body() {
    let session = Session::new(Stored::default());
    let prefix = pl_core::chat::presentation_prefix("attempt");
    let id = pl_core::chat::presentation_item_id(
        "attempt",
        "message-1",
        Some(PresentationPart::OutputText(0)),
    );
    session
        .publish_preview(identified(&id, 1, 1, "realtime ", false))
        .unwrap();
    session
        .publish(identified(&id, 1, 2, "realtime delayed first token", true))
        .unwrap();
    session.confirm_saved(&id, 2);

    // A slow producer publishes one more revision of the identity after the acknowledgement and
    // before (or during) the release; it is refused, so it cannot re-open the committed row.
    assert!(matches!(
        session.publish_preview(identified(&id, 1, 3, "realtime delayed", false)),
        Err(ChatError::Conflict(_))
    ));
    session.drop_previews_with_prefix(&prefix);

    let view = session.open_chat(ChatFocus::Latest).await.unwrap();
    let snapshot = view.snapshot();
    assert_eq!(snapshot.items.len(), 1);
    assert_eq!(snapshot.items[0].revision, 2);
    assert_eq!(
        body_text(&snapshot.items[0]),
        "realtime delayed first token"
    );
    assert_eq!(snapshot.items[0].order, 1);
    assert!(snapshot.items[0].is_terminal());
}

/// The presentation identity is durable: it is the row key history stores, the identity a late
/// preview is rejected against and the identity a terminal item finalizes. The builder assembles it
/// in one allocation instead of formatting an intermediate prefix, so this pins the exact bytes for
/// every part shape and that the prefix helper stays a genuine prefix of the item identity.
#[test]
fn presentation_item_identity_bytes_are_stable_for_every_part_shape() {
    let attempt = "48:input:input-b3fb437ee05a2d064704c414642eeb2a:3:0";
    let prefix = format!("model:{}:{attempt}:presentation:item:", attempt.len());
    assert_eq!(pl_core::chat::presentation_prefix(attempt), prefix);

    let head = format!("{prefix}{}:message-1", "message-1".len());
    assert_eq!(
        pl_core::chat::presentation_item_id(
            attempt,
            "message-1",
            Some(PresentationPart::OutputText(0)),
        ),
        format!("{head}:text:0"),
    );
    assert_eq!(
        pl_core::chat::presentation_item_id(
            attempt,
            "message-1",
            Some(PresentationPart::ReasoningText(2)),
        ),
        format!("{head}:reasoning:2"),
    );
    assert_eq!(
        pl_core::chat::presentation_item_id(
            attempt,
            "message-1",
            Some(PresentationPart::SummaryText(1)),
        ),
        format!("{head}:summary:1"),
    );
    // An identity with no part is still the same item under the same reserved name.
    assert_eq!(
        pl_core::chat::presentation_item_id(attempt, "message-1", None),
        format!("{head}:empty"),
    );
}
