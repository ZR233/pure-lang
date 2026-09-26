//! Bounded, storage-independent reading windows over one session's timeline.

use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet, VecDeque},
    fmt::{self, Write as _},
    future::Future,
    sync::{Arc, Mutex, Weak},
    time::Duration,
};

use futures::future::BoxFuture;
use tokio::sync::watch;

use crate::model::ContentBlock;

const RECENT_ITEMS: usize = 100;
const INITIAL_ITEMS: usize = 32;
const WINDOW_ITEMS: usize = 96;
const PAGE_ITEMS: usize = 32;
const RECENT_BYTES: usize = 8 * 1024 * 1024;
const ITEM_PREVIEW_BYTES: usize = 256 * 1024;
/// Bound on the terminal identities whose full marker the session keeps after an item leaves the
/// recent cache.
///
/// Each marker is tiny compared with the body it protects. When the bound is exceeded the marker
/// with the smallest order is evicted and its order is folded into the session's `terminal_floor`,
/// so eviction never reopens the identity: order slots are allocated strictly increasing and never
/// reused for a different identity, so no later publication can legitimately carry an order at or
/// below the floor. Living on a live reservation or on a cold history read stays legal.
const TERMINAL_ITEMS: usize = 4096;
/// Bound on how long ordinary already-started text is merged into one delivered frame.
///
/// The deadline is anchored when a lock-free waiter first observes an ordinary coalesced change and
/// is **not** restarted by a later delta, so a running stream is delivered at a fixed cadence rather
/// than being pushed out token by token. Boundaries (a first byte, an execution terminal admission,
/// a save acknowledgement, a field removal, an authoritative replacement or a structural change)
/// never wait for it: they are published on the urgent waterline and delivered at once.
const COALESCE_WINDOW: Duration = Duration::from_millis(33);

/// A provider part's identity does not depend on late output indexes or part IDs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PresentationPart {
    OutputText(u32),
    ReasoningText(u32),
    SummaryText(u32),
}

pub fn presentation_prefix(attempt_id: &str) -> String {
    let mut prefix = String::with_capacity(attempt_id.len() + PRESENTATION_PREFIX_BYTES);
    write_presentation_prefix(&mut prefix, attempt_id);
    prefix
}

pub fn presentation_item_id(
    attempt_id: &str,
    provider_item_id: &str,
    part: Option<PresentationPart>,
) -> String {
    // The identity of one streamed provider part is rebuilt once per part per preview frame, so it is
    // assembled in one allocation instead of formatting an intermediate prefix string and two
    // temporary concatenations. The bytes are unchanged.
    let mut base = String::with_capacity(
        PRESENTATION_PREFIX_BYTES + attempt_id.len() + provider_item_id.len(),
    );
    write_presentation_prefix(&mut base, attempt_id);
    let _ = write!(base, "{}:{provider_item_id}", provider_item_id.len());
    match part {
        Some(PresentationPart::OutputText(index)) => {
            let _ = write!(base, ":text:{index}");
        }
        Some(PresentationPart::ReasoningText(index)) => {
            let _ = write!(base, ":reasoning:{index}");
        }
        Some(PresentationPart::SummaryText(index)) => {
            let _ = write!(base, ":summary:{index}");
        }
        None => base.push_str(":empty"),
    }
    base
}

/// Fixed bytes of the presentation prefix that are neither the length digits nor the attempt id.
const PRESENTATION_PREFIX_BYTES: usize = "model::presentation:item:".len();

/// Appends the presentation prefix of one attempt without building an intermediate string.
fn write_presentation_prefix(id: &mut String, attempt_id: &str) {
    let _ = write!(
        id,
        "model:{}:{attempt_id}:presentation:item:",
        attempt_id.len()
    );
}

/// Stable identity of one content region inside a [`ChatItem`].
///
/// Core never encodes a protocol tag here. The host interprets these identities and maps them to
/// its own field (for example a `ThreadContentField`), so a window can address the streamed text of
/// one provider part without decoding or re-encoding a whole payload string.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ChatField {
    /// The item's primary body: a non-streaming item, or one provider part that maps 1:1 to it.
    Body,
    /// One provider content part identified by the model port's own [`PresentationPart`].
    Part(PresentationPart),
    /// A host-defined content region: an opaque, product-agnostic domain key.
    ///
    /// Core never interprets the key or infers a domain from the field's text, so a product can give
    /// distinct streaming content that is not a provider presentation part (for example tool
    /// arguments versus a tool result) its own stable field without decoding the body or stuffing a
    /// dynamic domain into `meta` and copying the whole item per token.
    Host(Arc<str>),
}

impl ChatField {
    /// Field identity for one provider content part.
    pub const fn part(part: PresentationPart) -> Self {
        Self::Part(part)
    }

    /// Field identity for one opaque host-defined content region.
    pub fn host(key: impl Into<Arc<str>>) -> Self {
        Self::Host(key.into())
    }
}

/// Field identity for the item's primary body.
pub const CHAT_BODY_FIELD: ChatField = ChatField::Body;

/// Execution terminal fact of one timeline item, independent of the save watermark.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ChatLifecycle {
    /// Still executing: a later revision of the same identity may still advance the content.
    Streaming,
    /// The producer will not advance this identity again. A late preview or a different body at this
    /// identity is rejected from this fact alone, independent of any save acknowledgement.
    Terminal,
}

impl ChatLifecycle {
    /// Reports whether the item reached its execution terminal fact.
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Terminal)
    }

    /// Reports whether the identity may still advance its content.
    pub const fn is_streaming(self) -> bool {
        matches!(self, Self::Streaming)
    }
}

/// One timeline item as a set of typed, independently versioned content fields.
///
/// `fields` holds the streamed text as immutable [`ContentBlock`]s shared with the producer, so an
/// update clones pointers instead of copying or JSON-encoding the whole body. `meta` is opaque,
/// product-owned static metadata (kind, timestamps ...) that the host writes once and only encodes
/// at its own boundary; it never holds the streamed text, so there is no second source for a body.
///
/// Four independent facts are in play, none of which substitutes for another:
///
/// - `revision` is this item's content version. It advances with the streamed content and is the
///   commit point a batched [`ViewChange::UpdateItem`] validates and applies once.
/// - [`ChatSnapshot::version`] is the session's window version. Every publication advances it, so it
///   is the continuity watermark a patch's `from`/`to` refer to, not any item's content version.
/// - `saved` is the save watermark: the history writer acknowledged this exact `revision` durable.
///   [`Session::confirm_saved`] only flips it, without changing identity, order, revision, body or
///   the execution lifecycle. It is separate from the terminal fact, so an item may be terminal and
///   still unsaved (its effect is not yet acknowledged) or saved while still streaming (a tool
///   invocation persisted before it finishes).
/// - [`ChatItem::lifecycle`] is the execution terminal fact: the producer will not advance the
///   content again. A terminal item is accepted without any save acknowledgement, and a late preview
///   is rejected from this fact alone, independent of how fast the writer acknowledges.
///
/// The model context is unrelated to this representation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatItem {
    pub item_id: String,
    pub turn_id: String,
    pub order: u64,
    /// Content version of this item. It advances with the streamed content and stays independent of
    /// the save or execution watermark.
    pub revision: u64,
    pub fields: BTreeMap<ChatField, Arc<ContentBlock>>,
    pub meta: Arc<[u8]>,
    /// Bytes omitted from the bounded preview of this item; zero means the fields are complete.
    pub omitted_bytes: u64,
    /// Save watermark: the history writer acknowledged this exact `revision` durable. Independent of
    /// [`ChatItem::lifecycle`].
    pub saved: bool,
    /// Execution terminal fact, independent of the content revision and of `saved`.
    pub lifecycle: ChatLifecycle,
}

impl ChatItem {
    /// Content block of one field, if this item carries it.
    pub fn field(&self, field: &ChatField) -> Option<&Arc<ContentBlock>> {
        self.fields.get(field)
    }

    /// Reports whether this item reached its execution terminal fact.
    pub fn is_terminal(&self) -> bool {
        self.lifecycle.is_terminal()
    }

    /// Reports whether the identity may still advance its content.
    pub fn is_streaming(&self) -> bool {
        self.lifecycle.is_streaming()
    }

    /// Reports whether the history writer acknowledged this exact revision durable.
    pub fn is_saved(&self) -> bool {
        self.saved
    }

    /// The item's primary body block, if it has one.
    pub fn body(&self) -> Option<&Arc<ContentBlock>> {
        self.fields.get(&ChatField::Body)
    }

    /// One provider part's block, if this item carries it.
    pub fn part(&self, part: PresentationPart) -> Option<&Arc<ContentBlock>> {
        self.fields.get(&ChatField::Part(part))
    }

    /// Bytes of streamed content, excluding the static metadata.
    fn content_bytes(&self) -> usize {
        self.fields.values().map(|block| block.len()).sum()
    }

    /// Bytes resident for this item, including the static metadata.
    fn resident_bytes(&self) -> usize {
        self.content_bytes().saturating_add(self.meta.len())
    }
}

/// Whether two items carry the same field content.
fn same_content(left: &ChatItem, right: &ChatItem) -> bool {
    left.meta == right.meta
        && left.fields.len() == right.fields.len()
        && left.fields.iter().all(|(field, block)| {
            right
                .fields
                .get(field)
                .is_some_and(|other| Arc::ptr_eq(block, other) || block == other)
        })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatFocus {
    Latest,
    Around(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Older,
    Newer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatQuery {
    Latest,
    Before(u64),
    After(u64),
    Around(u64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryPage {
    pub items: Vec<ChatItem>,
    pub has_older: bool,
    pub has_newer: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ChatError {
    #[error("chat item has a conflicting identity, order or revision: {0}")]
    Conflict(String),
    #[error("chat item order exceeds the persistent store's range")]
    OrderExhausted,
    #[error("chat order allocator was not initialized before synchronous publication")]
    OrderNotInitialized,
    #[error("chat anchor not found: {0}")]
    MissingAnchor(String),
    #[error("chat history failed: {0}")]
    History(#[source] Box<dyn std::error::Error + Send + Sync>),
}

/// Host persistence is the only source of older rows. It must use keyset queries; a
/// short page is not evidence that either end of the history has been reached.
pub trait ChatHistory: Send + Sync + fmt::Debug + 'static {
    /// Project a bounded visible copy without changing the canonical item.
    ///
    /// The default bounds every content field so the whole item fits [`ITEM_PREVIEW_BYTES`],
    /// reusing the shared block prefix instead of re-materializing the text. `omitted_bytes`
    /// records what was dropped so a host can request the complete body by identity later. A
    /// codec-aware host overrides this when its own encoding has per-item limits.
    fn bounded(&self, item: &ChatItem) -> Result<ChatItem, ChatError> {
        if item.omitted_bytes != 0 {
            return Ok(item.clone());
        }
        let total = item.content_bytes();
        if total <= ITEM_PREVIEW_BYTES {
            return Ok(item.clone());
        }
        let mut preview = item.clone();
        let mut remaining = ITEM_PREVIEW_BYTES;
        let mut kept = 0usize;
        for block in preview.fields.values_mut() {
            let (bounded, bytes) = ContentBlock::prefix_at(&*block, remaining);
            *block = bounded;
            kept = kept.saturating_add(bytes);
            remaining = remaining.saturating_sub(bytes);
        }
        preview.omitted_bytes = item
            .omitted_bytes
            .saturating_add((total.saturating_sub(kept)) as u64);
        Ok(preview)
    }

    /// Highest committed or already allocated order. A cold session reads this once before
    /// admitting a new identity; subsequent allocations do not perform storage IO.
    fn latest_allocated_order(&self) -> impl Future<Output = Result<u64, ChatError>> + Send;

    fn page(
        &self,
        query: ChatQuery,
        limit: usize,
    ) -> impl Future<Output = Result<HistoryPage, ChatError>> + Send;

    /// The complete canonical item, never a bounded preview.
    fn item(
        &self,
        item_id: &str,
    ) -> impl Future<Output = Result<Option<ChatItem>, ChatError>> + Send;
}

trait ErasedChatHistory: Send + Sync + fmt::Debug {
    fn bounded(&self, item: &ChatItem) -> Result<ChatItem, ChatError>;
    fn latest_allocated_order(&self) -> BoxFuture<'_, Result<u64, ChatError>>;
    fn page(&self, query: ChatQuery, limit: usize)
    -> BoxFuture<'_, Result<HistoryPage, ChatError>>;
    fn item<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<Option<ChatItem>, ChatError>>;
}

impl<T: ChatHistory> ErasedChatHistory for T {
    fn bounded(&self, item: &ChatItem) -> Result<ChatItem, ChatError> {
        ChatHistory::bounded(self, item)
    }

    fn latest_allocated_order(&self) -> BoxFuture<'_, Result<u64, ChatError>> {
        Box::pin(ChatHistory::latest_allocated_order(self))
    }

    fn page(
        &self,
        query: ChatQuery,
        limit: usize,
    ) -> BoxFuture<'_, Result<HistoryPage, ChatError>> {
        Box::pin(ChatHistory::page(self, query, limit))
    }

    fn item<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<Option<ChatItem>, ChatError>> {
        Box::pin(ChatHistory::item(self, id))
    }
}

#[derive(Default)]
struct TimelineState {
    recent: VecDeque<ChatItem>,
    recent_bytes: usize,
    // The reliable effect owner is elsewhere. These references remain until the exact
    // revision is committed, even if the corresponding item leaves every view.
    unsaved: BTreeMap<String, ChatItem>,
    unsaved_previews: BTreeMap<String, ChatItem>,
    unsaved_order: BTreeMap<u64, String>,
    // Large speculative previews can be read while the model is still streaming. They
    // are not reliable history. Each entry shares the producer's immutable content block, so a
    // window can show the complete in-flight body instead of a bounded preview; the number of
    // live identities is bounded upstream and the entries are released on terminal admission.
    active_previews: BTreeMap<String, ChatItem>,
    allocated: BTreeMap<String, u64>,
    allocated_order: BTreeMap<u64, String>,
    // Terminal identities recalled after their item leaves `recent`, bounded to `TERMINAL_ITEMS`.
    // Each entry keeps only the id and its terminal revision, so a late preview for a committed
    // identity that was cache-evicted is still rejected instead of resurrecting a body.
    terminal: BTreeMap<String, u64>,
    // The same markers indexed by order for bounded eviction: the smallest order is dropped first.
    terminal_order: BTreeMap<u64, String>,
    // Monotonic order watermark: the greatest order whose terminal marker was evicted from
    // `terminal`. Order slots are allocated strictly increasing and never reused, so any
    // publication at or below this floor that has no live reservation is a late update for an
    // already committed identity rather than a legitimate new one.
    terminal_floor: u64,
    highest_order: u64,
    version: u64,
    // Monotone waterline of the greatest version produced by a frame boundary that must be delivered
    // without coalescing (a new identity, an execution terminal admission, a save acknowledgement, a
    // field removal, an authoritative replacement or a structural window change). A lock-free waiter
    // compares it against its own delivered version to tell a pending boundary (`urgent > seen`) from
    // ordinary already-started text, without building a snapshot or a diff.
    urgent: u64,
}

/// The minimal product-agnostic progress fact one session publishes to its watchers.
///
/// `version` is the monotone window version that patch `from`/`to` watermarks refer to. `urgent` is
/// the boundary waterline described on [`TimelineState::urgent`]. Both are plain numbers, so a
/// watcher can decide readiness outside every lock while inspecting neither a body nor a diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ChatProgress {
    version: u64,
    urgent: u64,
}

/// Advances a session's window version and folds in the boundary waterline.
///
/// `urgent` marks a publication that must not wait for the coalescing window. The returned fact is
/// what a [`SessionInner::changed`] watcher observes; it carries only versions, never content.
fn bump_version(state: &mut TimelineState, urgent: bool) -> ChatProgress {
    state.version = state.version.wrapping_add(1);
    if urgent {
        state.urgent = state.version;
    }
    ChatProgress {
        version: state.version,
        urgent: state.urgent,
    }
}

struct SessionInner {
    history: Arc<dyn ErasedChatHistory>,
    order_seed: tokio::sync::OnceCell<u64>,
    timeline: Mutex<TimelineState>,
    windows: Mutex<Vec<Weak<Mutex<Window>>>>,
    changed: watch::Sender<ChatProgress>,
}

/// Shared timeline data for one session. Opening a view neither starts a model nor
/// transfers the owner of an uncommitted effect to a GUI subscriber.
#[derive(Clone)]
pub struct Session(Arc<SessionInner>);

/// A registry reference that does not keep an idle history connection open.
#[derive(Debug, Clone)]
pub struct WeakSession(Weak<SessionInner>);

impl WeakSession {
    pub fn upgrade(&self) -> Option<Session> {
        self.0.upgrade().map(Session)
    }
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session").finish_non_exhaustive()
    }
}

impl Session {
    pub fn new(history: impl ChatHistory) -> Self {
        let (changed, _) = watch::channel(ChatProgress {
            version: 0,
            urgent: 0,
        });
        Self(Arc::new(SessionInner {
            history: Arc::new(history),
            order_seed: tokio::sync::OnceCell::new(),
            timeline: Mutex::new(TimelineState::default()),
            windows: Mutex::new(Vec::new()),
            changed,
        }))
    }

    pub fn downgrade(&self) -> WeakSession {
        WeakSession(Arc::downgrade(&self.0))
    }

    /// Returns an order already assigned to an item, without allocating a new one.
    pub fn assigned_order(&self, item_id: &str) -> Option<u64> {
        let state = self
            .0
            .timeline
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        order_for_identity(&state, item_id)
    }

    /// Whether this session already holds one identity's terminal fact.
    ///
    /// True once the identity is terminal (or remembered by its terminal marker after its body left
    /// the window), and never cleared by a save acknowledgement. It stays false while an
    /// acknowledged identity still runs (a background tool result keeps streaming). A producer uses
    /// it to decide whether a late speculative frame is new content or a stale update to a row
    /// durable history owns: the window position and body of a committed identity are final, so a
    /// preview for it is ignored instead of replacing — and later releasing — committed content.
    pub fn is_committed(&self, item_id: &str) -> bool {
        let state = self
            .0
            .timeline
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        committed_identity(&state, item_id)
    }

    /// Reads the durable order ceiling once while activating a producer. This does not
    /// create a history database or start the model; after it succeeds the synchronous
    /// admission path can allocate orders without storage IO.
    pub async fn initialize_order_allocator(&self) -> Result<(), ChatError> {
        self.0
            .order_seed
            .get_or_try_init(|| self.0.history.latest_allocated_order())
            .await?;
        Ok(())
    }

    /// Allocates an order during synchronous effect admission. The producer must first
    /// initialize this session's allocator; repeated identities retain their order.
    pub fn reserve_order_in_memory(&self, item_id: &str) -> Result<u64, ChatError> {
        let seed = *self
            .0
            .order_seed
            .get()
            .ok_or(ChatError::OrderNotInitialized)?;
        let mut state = self
            .0
            .timeline
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(order) = order_for_identity(&state, item_id) {
            return Ok(order);
        }
        let order = state
            .highest_order
            .max(seed)
            .checked_add(1)
            .filter(|order| *order <= i64::MAX as u64)
            .ok_or(ChatError::OrderExhausted)?;
        state.highest_order = order;
        state.allocated.insert(item_id.to_owned(), order);
        state.allocated_order.insert(order, item_id.to_owned());
        Ok(order)
    }

    /// Reserves a session-local order from asynchronous clients. The first call reads
    /// the persisted ceiling; subsequent calls allocate entirely in memory.
    pub async fn reserve_order(&self, item_id: &str) -> Result<u64, ChatError> {
        {
            let state = self
                .0
                .timeline
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if let Some(order) = order_for_identity(&state, item_id) {
                return Ok(order);
            }
        }
        self.initialize_order_allocator().await?;
        self.reserve_order_in_memory(item_id)
    }

    /// Abandons a reservation whose item never became visible. The number is not reused.
    pub fn release_unpublished_order(&self, item_id: &str) {
        let mut state = self
            .0
            .timeline
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        // A committed identity is never "unpublished": its order belongs to the durable row, so
        // releasing it here would let a later preview claim a placement the history already owns.
        if committed_content(&state, item_id) {
            return;
        }
        if let Some(order) = state.allocated.remove(item_id) {
            state.allocated_order.remove(&order);
        }
    }

    /// Updates an item in place. A duplicate revision is accepted only with identical data.
    /// The caller retains its immutable effect until the history writer confirms it.
    pub fn publish(&self, item: ChatItem) -> Result<(), ChatError> {
        self.publish_inner(item, true)
    }

    /// Live provider previews have no accepted history effect yet. They are visible
    /// while the request runs, but cannot claim reliable-unsaved ownership.
    pub fn publish_preview(&self, item: ChatItem) -> Result<(), ChatError> {
        self.publish_inner(item, false)
    }

    pub fn drop_preview(&self, item_id: &str) {
        self.drop_previews_matching(|candidate| candidate == item_id);
    }

    /// Releases speculative identities after the host has committed the terminal effect.
    /// Never call this during a failed write: a retry needs the same reserved orders.
    pub fn drop_previews_with_prefix(&self, prefix: &str) {
        self.drop_previews_matching(|candidate| candidate.starts_with(prefix));
    }

    /// Releases speculative identities after a commit, except the identities it just confirmed.
    ///
    /// The host writes exactly one batch per commit and knows which identities that transaction
    /// saved. Excluding them makes the release correct even when the session itself never saw the
    /// terminal publication — an effect the owner installed behind is folded into the snapshot
    /// without a live publish — so a confirmed body and its position are never released as if they
    /// were still speculative.
    pub fn drop_previews_with_prefix_except<'a>(
        &self,
        prefix: &str,
        confirmed: impl IntoIterator<Item = &'a str>,
    ) {
        let confirmed: BTreeSet<&str> = confirmed.into_iter().collect();
        self.drop_previews_matching(|candidate| {
            candidate.starts_with(prefix) && !confirmed.contains(candidate)
        });
    }

    fn drop_previews_matching(&self, matches: impl Fn(&str) -> bool) {
        let mut state = self
            .0
            .timeline
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        // Releasing speculative identities may only ever remove a row this session never terminated.
        // The writer releases an attempt's previews right after it confirms the batch it just saved,
        // and a slow producer can publish one more revision of that same identity between the
        // confirmation and this release. Dropping it would erase committed content from every window
        // and discard its position even though durable history owns that exact identity. The terminal
        // fact therefore protects an identity here exactly as it protects it from a late preview.
        let mut candidates: BTreeSet<String> = state.allocated.keys().cloned().collect();
        candidates.extend(state.recent.iter().map(|item| item.item_id.clone()));
        let abandoned: BTreeSet<String> = candidates
            .into_iter()
            .filter(|id| {
                matches(id) && !state.unsaved.contains_key(id) && !committed_content(&state, id)
            })
            .collect();
        if abandoned.is_empty() {
            return;
        }
        for id in &abandoned {
            if let Some(order) = state.allocated.remove(id) {
                state.allocated_order.remove(&order);
            }
            state.active_previews.remove(id);
        }
        let old_len = state.recent.len();
        state
            .recent
            .retain(|item| !abandoned.contains(&item.item_id));
        state.recent_bytes = state.recent.iter().map(ChatItem::resident_bytes).sum();
        let mut windows = self
            .0
            .windows
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let mut visible_changed = false;
        windows.retain(|window| {
            let Some(window) = window.upgrade() else {
                return false;
            };
            let mut window = window.lock().unwrap_or_else(|error| error.into_inner());
            let old_len = window.items.len();
            window
                .items
                .retain(|item| !abandoned.contains(&item.item_id));
            visible_changed |= window.items.len() != old_len;
            true
        });
        if state.recent.len() != old_len || visible_changed {
            let progress = bump_version(&mut state, true);
            self.0.changed.send_replace(progress);
        }
    }

    fn publish_inner(&self, item: ChatItem, reliable: bool) -> Result<(), ChatError> {
        // A reliable publication owns the uncommitted effect (whether or not the identity is already
        // terminal), so it must carry complete content rather than a bounded preview. A terminal item
        // with a partial body is rejected here too, without waiting for the writer.
        if reliable && !item.saved && item.omitted_bytes != 0 {
            return Err(ChatError::Conflict(item.item_id));
        }
        // The host codec may bound or rewrite the item, so the visible copy is built before the
        // timeline lock: a concurrent publication never waits behind codec work, and no database or
        // codec call runs under the lock.
        let visible = self.0.history.bounded(&item)?;
        let mut state = self
            .0
            .timeline
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if item.order == 0 || item.order > i64::MAX as u64 {
            return Err(ChatError::Conflict(item.item_id));
        }
        // A speculative preview never reopens or overwrites an identity this session already holds as
        // terminal: the committed body, its window position and its terminal revision are final. That
        // decision belongs to the two branches that already own it below — the resident row and the
        // terminal marker — so a late preview keeps reporting the same typed conflict the public
        // contract has always reported, and, because it never replaces the committed row, the writer's
        // later preview release cannot erase that row either.
        if state
            .unsaved_order
            .get(&item.order)
            .is_some_and(|existing| existing != &item.item_id)
            || state
                .allocated_order
                .get(&item.order)
                .is_some_and(|existing| existing != &item.item_id)
            || state
                .allocated
                .get(&item.item_id)
                .is_some_and(|order| *order != item.order)
            || state
                .recent
                .iter()
                .any(|existing| existing.order == item.order && existing.item_id != item.item_id)
        {
            return Err(ChatError::Conflict(item.item_id));
        }
        let resident = state
            .unsaved
            .get(&item.item_id)
            .or_else(|| state.recent.iter().find(|old| old.item_id == item.item_id));
        // Whether this admission is a frame boundary that must not wait for the coalescing window.
        // A pure append-only advance of an already-known identity is ordinary text; everything else
        // (a new identity, a terminal admission, a save receipt, new static metadata, a removed or
        // authoritatively replaced field) is a boundary the host must see at once.
        let boundary = publication_is_boundary(resident, &item);
        if let Some(previous) = resident {
            if previous.order != item.order
                || (!previous.turn_id.is_empty()
                    && previous.turn_id != item.turn_id
                    && !((item.saved && !previous.saved)
                        || (item.is_terminal() && previous.is_streaming())))
            {
                return Err(ChatError::Conflict(item.item_id));
            }
            if previous.revision > item.revision {
                return Ok(());
            }
            // A streaming preview is never newer content for a terminal identity: the producer cannot
            // advance it again. A strictly older revision was a stale duplicate and was ignored above,
            // but a preview at the terminal revision or beyond is a late update and is rejected instead
            // of replacing — and, through the writer's later release, erasing — the committed row.
            if !reliable && item.is_streaming() && previous.is_terminal() {
                return Err(ChatError::Conflict(item.item_id));
            }
            if previous.revision == item.revision {
                if ((previous.omitted_bytes == 0
                    && item.omitted_bytes == 0
                    && !same_content(previous, &item))
                    || previous.turn_id != item.turn_id)
                    && (reliable || previous.saved || state.unsaved.contains_key(&item.item_id))
                    && !((item.saved && !previous.saved)
                        || (item.is_terminal() && previous.is_streaming()))
                {
                    return Err(ChatError::Conflict(item.item_id));
                }
                if previous.saved && !item.saved {
                    return Ok(());
                }
                if previous.is_terminal() && item.is_streaming() {
                    return Ok(());
                }
            }
        } else if let Some(&terminal_revision) = state.terminal.get(&item.item_id) {
            // The terminal identity already left the recent cache, so memory holds no body to compare
            // against. An older revision is a stale duplicate and is ignored; a preview at the
            // terminal revision or any newer revision is a late update for a committed identity and
            // is rejected instead of resurrecting a stale body. Durable history stays the ultimate
            // owner beyond this bound.
            if item.revision < terminal_revision {
                return Ok(());
            }
            if item.revision > terminal_revision
                || (item.revision == terminal_revision && item.is_streaming())
            {
                return Err(ChatError::Conflict(item.item_id));
            }
        } else if item.order <= state.terminal_floor && !state.allocated.contains_key(&item.item_id)
        {
            // The marker for this order was evicted, but an order slot is allocated strictly
            // increasing and never reused for another identity. A publication at or below the
            // evicted-terminal floor without a live reservation can therefore only be a late update
            // for an identity this session already committed, so it is rejected rather than
            // resurrecting a stale body. Durable history stays the ultimate owner of the content.
            return Err(ChatError::Conflict(item.item_id));
        }
        state.highest_order = state.highest_order.max(item.order);
        state.active_previews.remove(&item.item_id);
        if item.is_terminal() {
            remember_terminal(&mut state, &item.item_id, item.order, item.revision);
        }
        // A speculative preview keeps its complete in-memory content so a window can render the
        // whole in-flight body. The entries share the producer's content block, so retaining them
        // does not keep a second full copy, and they are released on terminal admission.
        if !reliable && item.is_streaming() && visible.omitted_bytes != 0 {
            state
                .active_previews
                .insert(item.item_id.clone(), item.clone());
        }
        if reliable || item.saved || item.is_terminal() {
            state.allocated.remove(&item.item_id);
            state.allocated_order.remove(&item.order);
        }
        state.recent.retain(|old| old.item_id != item.item_id);
        state.recent_bytes = state.recent.iter().map(ChatItem::resident_bytes).sum();
        // The reliable uncommitted owner is retained until the exact revision is acknowledged, so a
        // terminal item that is not yet saved still keeps its whole body for the writer.
        if !item.saved && reliable {
            state.unsaved_order.insert(item.order, item.item_id.clone());
            state.unsaved.insert(item.item_id.clone(), item.clone());
            state
                .unsaved_previews
                .insert(item.item_id.clone(), visible.clone());
        } else if state
            .unsaved
            .get(&item.item_id)
            .is_some_and(|previous| previous.revision == item.revision)
        {
            state.unsaved_order.remove(&item.order);
            state.unsaved.remove(&item.item_id);
            state.unsaved_previews.remove(&item.item_id);
        }
        state.recent_bytes = state.recent_bytes.saturating_add(visible.resident_bytes());
        state.recent.push_back(visible.clone());
        state
            .recent
            .make_contiguous()
            .sort_by_key(|item| item.order);
        while state.recent.len() > RECENT_ITEMS || state.recent_bytes > RECENT_BYTES {
            let Some(old) = state.recent.pop_front() else {
                break;
            };
            state.recent_bytes -= old.resident_bytes();
        }
        // A window shows the complete in-memory body while the item is still in flight, and the
        // bounded preview only once it is durable history — unless the window asked for this
        // identity's complete body, in which case it keeps following the complete shared fields
        // directly instead of falling back to the preview on each new revision.
        let resident_full = item.is_streaming()
            && (state.unsaved.contains_key(&item.item_id)
                || state.active_previews.contains_key(&item.item_id));
        self.publish_visible(&item, &visible, resident_full);
        let progress = bump_version(&mut state, boundary);
        self.0.changed.send_replace(progress);
        Ok(())
    }

    /// An older writer acknowledgement cannot mark a newer revision as durable.
    ///
    /// The history writer only confirms the exact identity and revision of a batch the Thread's live
    /// projection already published, so the confirmation this consumes is the durable receipt of a
    /// body the session holds. This flips the save watermark `saved` only: it never advances the
    /// execution lifecycle, never releases an identity that is still producing content, and never
    /// rejects a later revision. A terminal item stays terminal whether or not it is acknowledged,
    /// and a streaming item stays streaming after acknowledgement.
    pub fn confirm_saved(&self, item_id: &str, revision: u64) {
        let mut state = self
            .0
            .timeline
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if state
            .unsaved
            .get(item_id)
            .is_some_and(|item| item.revision == revision)
            && let Some(item) = state.unsaved.remove(item_id)
        {
            state.unsaved_order.remove(&item.order);
            state.unsaved_previews.remove(item_id);
        }
        let mut changed = false;
        if let Some(item) = state
            .recent
            .iter_mut()
            .find(|item| item.item_id == item_id && item.revision == revision && !item.saved)
        {
            item.saved = true;
            changed = true;
        }
        self.update_visible(item_id, |item| {
            if item.revision == revision && !item.saved {
                item.saved = true;
                changed = true;
            }
        });
        if changed {
            // A save acknowledgement is a frame boundary, not ordinary started text.
            let progress = bump_version(&mut state, true);
            self.0.changed.send_replace(progress);
        }
    }

    pub async fn open_chat(&self, focus: ChatFocus) -> Result<ChatView, ChatError> {
        let view = ChatView {
            session: self.clone(),
            window: Arc::new(Mutex::new(Window {
                focus: ChatFocus::Latest,
                items: Vec::new(),
                complete: BTreeMap::new(),
                version: 0,
                has_older: false,
                has_newer: false,
                generation: 0,
            })),
        };
        self.0
            .windows
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(Arc::downgrade(&view.window));
        view.focus(focus).await?;
        Ok(view)
    }

    fn memory_page(&self, query: ChatQuery, limit: usize) -> Vec<ChatItem> {
        let state = self
            .0
            .timeline
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        // Prefer the complete in-flight content over the bounded preview when the item is resident.
        let unsaved = |item_id: &String| {
            resident_item(&state, item_id).or_else(|| state.unsaved_previews.get(item_id).cloned())
        };
        let pending: Vec<_> = match query {
            ChatQuery::Latest => state
                .unsaved_order
                .values()
                .rev()
                .take(limit)
                .filter_map(unsaved)
                .collect(),
            ChatQuery::Before(anchor) => state
                .unsaved_order
                .range(..anchor)
                .rev()
                .map(|(_, id)| id)
                .take(limit)
                .filter_map(unsaved)
                .collect(),
            ChatQuery::After(anchor) => state
                .unsaved_order
                .range((
                    std::ops::Bound::Excluded(anchor),
                    std::ops::Bound::Unbounded,
                ))
                .map(|(_, id)| id)
                .take(limit)
                .filter_map(unsaved)
                .collect(),
            ChatQuery::Around(anchor) => state
                .unsaved_order
                .range(..anchor)
                .rev()
                .map(|(_, id)| id)
                .take(limit / 2)
                .chain(
                    state
                        .unsaved_order
                        .range(anchor..)
                        .map(|(_, id)| id)
                        .take(limit),
                )
                .filter_map(unsaved)
                .collect(),
        };
        let mut items = merge_items(state.recent.iter().cloned().chain(pending));
        if let ChatQuery::Before(anchor) | ChatQuery::After(anchor) = query {
            items.retain(|item| match query {
                ChatQuery::Before(_) => item.order < anchor,
                _ => item.order > anchor,
            });
        }
        match query {
            ChatQuery::Latest | ChatQuery::Before(_) => {
                items = items.into_iter().rev().take(limit).collect();
                items.reverse();
            }
            ChatQuery::After(_) => items.truncate(limit),
            ChatQuery::Around(anchor) => {
                let before = items.partition_point(|item| item.order < anchor);
                let start = before.saturating_sub(limit / 2);
                items = items.into_iter().skip(start).take(limit).collect();
            }
        }
        items
    }

    fn memory_item(&self, item_id: &str) -> Option<ChatItem> {
        let state = self
            .0
            .timeline
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state
            .unsaved
            .get(item_id)
            .or_else(|| state.active_previews.get(item_id))
            .or_else(|| state.recent.iter().find(|item| item.item_id == item_id))
            .cloned()
    }

    /// Reads the canonical item, including uncommitted facts owned by this session.
    pub async fn read_item(&self, item_id: &str) -> Result<Option<ChatItem>, ChatError> {
        if let Some(item) = self.memory_item(item_id) {
            if item.omitted_bytes == 0 {
                return Ok(Some(item));
            }
            return self
                .0
                .history
                .item(item_id)
                .await?
                .map(Some)
                .ok_or_else(|| {
                    ChatError::History(
                        std::io::Error::other(format!(
                            "full chat item {item_id} is not available yet"
                        ))
                        .into(),
                    )
                });
        }
        self.0.history.item(item_id).await
    }

    fn memory_preview_item(&self, item_id: &str) -> Option<ChatItem> {
        let state = self
            .0
            .timeline
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        resident_item(&state, item_id)
            .or_else(|| state.unsaved_previews.get(item_id).cloned())
            .or_else(|| {
                state
                    .recent
                    .iter()
                    .find(|item| item.item_id == item_id)
                    .cloned()
            })
    }

    fn hydrate(&self, items: &[ChatItem]) {
        let mut state = self
            .0
            .timeline
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        for item in items {
            if state.unsaved.contains_key(&item.item_id)
                || state.recent.iter().any(|old| old.item_id == item.item_id)
            {
                continue;
            }
            state.recent_bytes = state.recent_bytes.saturating_add(item.resident_bytes());
            state.recent.push_back(item.clone());
        }
        state
            .recent
            .make_contiguous()
            .sort_by_key(|item| item.order);
        while state.recent.len() > RECENT_ITEMS || state.recent_bytes > RECENT_BYTES {
            let Some(old) = state.recent.pop_front() else {
                break;
            };
            state.recent_bytes -= old.resident_bytes();
        }
    }

    fn bounded_page(&self, mut page: HistoryPage) -> Result<HistoryPage, ChatError> {
        for item in &mut page.items {
            *item = self.0.history.bounded(item)?;
        }
        Ok(page)
    }

    fn notify(&self) {
        let mut state = self
            .0
            .timeline
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        // A focus or window-move change is structural and must never wait for text coalescing.
        let progress = bump_version(&mut state, true);
        self.0.changed.send_replace(progress);
    }

    // Lock order is timeline -> window registry -> view, matching snapshot's
    // timeline -> view order; database IO is never performed under these locks.
    fn update_visible(&self, item_id: &str, mut update: impl FnMut(&mut ChatItem)) {
        let mut windows = self
            .0
            .windows
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        windows.retain(|window| {
            let Some(window) = window.upgrade() else {
                return false;
            };
            let mut window = window.lock().unwrap_or_else(|error| error.into_inner());
            if let Some(item) = window.items.iter_mut().find(|item| item.item_id == item_id) {
                update(item);
            }
            true
        });
    }

    /// Mirrors one publication into every open window, honoring each window's complete-body intent.
    ///
    /// A window that has requested this identity's complete body (its `complete` map holds a body for
    /// it) keeps following it: the publication — whose fields are the producer's shared complete
    /// [`ContentBlock`]s — becomes the visible body and refreshes the retained body, so the next
    /// revision is delivered as a typed append along the same shared chain instead of falling back to
    /// a bounded preview or performing a storage read. The retained body is refreshed at the
    /// publication's own authoritative revision and carries the publication's own save, lifecycle and
    /// static facts, so following never rolls back an independent timeline fact. Every other window
    /// keeps the complete in-flight body when `resident_full` (a streaming effect this session still
    /// owns), and the bounded preview otherwise. A window that no longer holds the identity is left
    /// untouched; the complete marker is released once the identity leaves the window by
    /// [`overlay_complete`], so a body is retained only while the window can still show it.
    ///
    /// Takes the timeline -> window registry -> view lock order like [`Self::update_visible`], so it
    /// must be called with the timeline lock already held, which the caller does. No storage IO or
    /// timeline re-lock happens under these locks.
    fn publish_visible(&self, item: &ChatItem, visible: &ChatItem, resident_full: bool) {
        let mut windows = self
            .0
            .windows
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        windows.retain(|window| {
            let Some(window) = window.upgrade() else {
                return false;
            };
            let mut window = window.lock().unwrap_or_else(|error| error.into_inner());
            let Some(index) = window
                .items
                .iter()
                .position(|candidate| candidate.item_id == item.item_id)
            else {
                return true;
            };
            if window.items[index].revision > item.revision {
                return true;
            }
            if window.complete.contains_key(&item.item_id) {
                window.items[index] = item.clone();
                retain_cached_body(&mut window.complete, item.clone());
            } else if resident_full {
                window.items[index] = item.clone();
            } else {
                window.items[index] = visible.clone();
            }
            true
        });
    }
}

fn order_for_identity(state: &TimelineState, item_id: &str) -> Option<u64> {
    state
        .allocated
        .get(item_id)
        .copied()
        .or_else(|| state.unsaved.get(item_id).map(|item| item.order))
        .or_else(|| {
            state
                .recent
                .iter()
                .find(|item| item.item_id == item_id)
                .map(|item| item.order)
        })
}

/// Whether one identity already reached its terminal fact in this session.
///
/// A committed identity is recalled by its terminal marker after the body left the window, or is
/// still resident as a terminal item: once true, the body and its window position are owned by
/// durable history and no speculative frame may replace them. A save acknowledgement is deliberately
/// *not* part of it — an acknowledged identity that is still running (a background tool result) keeps
/// streaming its content, and only its terminal fact makes a late preview stale.
fn committed_identity(state: &TimelineState, item_id: &str) -> bool {
    state.terminal.contains_key(item_id)
        || state
            .unsaved
            .get(item_id)
            .is_some_and(ChatItem::is_terminal)
        || state
            .recent
            .iter()
            .any(|item| item.item_id == item_id && item.is_terminal())
}

/// Whether one identity's row in this session must never be released as a stale preview.
///
/// It is terminal (committed content) or acknowledged (`saved`): the writer already confirmed that
/// exact revision durable, so the row is history rather than a speculative frame — even while a
/// running tool keeps streaming newer revisions of the same identity.
fn committed_content(state: &TimelineState, item_id: &str) -> bool {
    committed_identity(state, item_id)
        || state.unsaved.get(item_id).is_some_and(|item| item.saved)
        || state
            .recent
            .iter()
            .any(|item| item.item_id == item_id && item.saved)
}

/// The complete in-memory copy of an in-flight identity, if the session still holds one.
///
/// Reliable effects keep their whole item until their revision is committed, and speculative
/// previews keep theirs until terminal admission; both share the producer's content block.
fn resident_item(state: &TimelineState, item_id: &str) -> Option<ChatItem> {
    state
        .unsaved
        .get(item_id)
        .or_else(|| state.active_previews.get(item_id))
        .cloned()
}

/// Recalls one terminal identity, evicting the smallest-order marker past `TERMINAL_ITEMS`.
///
/// Each marker keeps only the id and its terminal revision, so it is tiny compared with the body it
/// protects. Eviction folds the dropped order into the session's `terminal_floor`, so an evicted
/// identity is still rejected by the order watermark instead of needing an unbounded tombstone.
fn remember_terminal(state: &mut TimelineState, item_id: &str, order: u64, revision: u64) {
    state.terminal.insert(item_id.to_owned(), revision);
    state.terminal_order.insert(order, item_id.to_owned());
    while state.terminal_order.len() > TERMINAL_ITEMS {
        let Some(evicted_order) = state.terminal_order.keys().next().copied() else {
            break;
        };
        let Some(evicted_id) = state.terminal_order.remove(&evicted_order) else {
            break;
        };
        state.terminal.remove(&evicted_id);
        state.terminal_floor = state.terminal_floor.max(evicted_order);
    }
}

struct Window {
    focus: ChatFocus,
    items: Vec<ChatItem>,
    /// Complete content for identities this view explicitly requested, retained only while the
    /// identity stays in the window and released when it leaves or the view is dropped.
    ///
    /// A present entry is also this view's standing **complete-body intent** for that identity: once
    /// asked for, every later revision of the identity keeps following the complete shared fields at
    /// publication time (see [`Session::publish_visible`]) instead of falling back to a bounded
    /// preview or a storage read, and the entry is refreshed to each publication's authoritative
    /// revision. The intent is per view and per identity, so it never materializes all of history and
    /// is released as soon as the identity leaves this view's window.
    complete: BTreeMap<String, ChatItem>,
    version: u64,
    has_older: bool,
    has_newer: bool,
    generation: u64,
}

#[derive(Clone)]
pub struct ChatView {
    session: Session,
    window: Arc<Mutex<Window>>,
}

impl fmt::Debug for ChatView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChatView").finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatSnapshot {
    pub focus: ChatFocus,
    /// Monotonic snapshot version of the session's window state. Every publication or window
    /// operation advances it, including changes to identities outside the visible items, so it is
    /// the continuity watermark a patch's `from`/`to` refer to. It is independent of any item's
    /// content revision and of the save watermark.
    pub version: u64,
    pub items: Vec<ChatItem>,
    pub has_older: bool,
    pub has_newer: bool,
}

/// Typed change for one content field, derived from the shared block lineage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldChange {
    /// The field is gone from the item; the consumer drops its copy.
    Remove,
    /// The content is unchanged while the item revision or bounded status advanced. A consumer must
    /// still commit the enclosing item revision, so a version-only frame is never an empty patch.
    Unchanged,
    /// The field still extends the consumer's baseline; these bytes are new.
    Append(String),
    /// The field no longer extends the consumer's baseline; use this whole authoritative block.
    Replace(Arc<ContentBlock>),
}

/// One field delta inside a batched [`ViewChange::UpdateItem`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldUpdate {
    pub field: ChatField,
    pub change: FieldChange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewChange {
    Splice {
        index: usize,
        remove: usize,
        items: Vec<ChatItem>,
    },
    /// Every content change of one identity, committed in a single step.
    ///
    /// The item's content version advances exactly once per delivered change: a consumer validates
    /// its local item against one `expected_revision`, applies every field delta, and only then
    /// commits `revision` and `omitted_bytes`. Several fields that changed together therefore never
    /// need a per-change revision bump. `expected_revision` and `revision` are content versions and
    /// are independent of the save or execution watermark. An `Append` is produced only while the
    /// consumer's delivered block is still on the field's shared prefix chain; when the chain was
    /// replaced (an authoritative body change, or a bounded baseline the producer extended past) the
    /// whole block is delivered instead, so a stale baseline never appends onto a body that changed
    /// under it. `omitted_bytes` reports the item's new bounded status, so a same-revision
    /// preview-to-complete upgrade updates it instead of leaving it stale. `saved` reports the item's
    /// new save watermark, so a save acknowledgement arrives as a typed update instead of an empty
    /// patch that would leave the consumer's watermark stale.
    UpdateItem {
        item_id: String,
        expected_revision: u64,
        revision: u64,
        omitted_bytes: u64,
        /// Save watermark of the item after this change, independent of `revision` and `lifecycle`.
        saved: bool,
        fields: Vec<FieldUpdate>,
    },
}

/// Whether a delivered frame is an immediate boundary or merged ordinary text.
///
/// Core derives this from authoritative window facts, so a host can rely on it without inventing a
/// protocol-specific priority: a boundary is a new identity, an execution terminal admission, a save
/// acknowledgement, a field removal, a structural window change or an authoritative body
/// replacement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatUpdatePriority {
    /// A boundary that must be delivered without any coalescing wait.
    Immediate,
    /// Ordinary already-started text (or a no-op) merged into one frame.
    Coalesced,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatUpdate {
    Reset(ChatSnapshot),
    Patch {
        from: u64,
        to: u64,
        changes: Vec<ViewChange>,
        has_newer: bool,
        /// Priority fact for this frame; `Reset` is always [`ChatUpdatePriority::Immediate`].
        priority: ChatUpdatePriority,
    },
}

impl ChatUpdate {
    /// Priority fact for this frame, product-agnostic and derived from authoritative window facts.
    pub fn priority(&self) -> ChatUpdatePriority {
        match self {
            Self::Reset(_) => ChatUpdatePriority::Immediate,
            Self::Patch { priority, .. } => *priority,
        }
    }
}

/// A cheap, clonable, cancel-safe readiness handle over one session's progress fact.
///
/// It never builds a snapshot or a diff and never holds the timeline or a window lock, so a caller
/// can put the wait outside every lock and take the typed frame later in a short synchronous
/// critical section. Cloning a handle yields an independent consumer: each one tracks its own
/// observed version through the `seen` argument rather than through shared receiver state.
#[derive(Clone)]
pub struct ChatWatch {
    changed: watch::Receiver<ChatProgress>,
}

impl ChatWatch {
    /// Current window version, read without building a snapshot or diff.
    pub fn current(&self) -> u64 {
        self.changed.borrow().version
    }

    /// Waits until the window version is no longer `seen` and returns it.
    ///
    /// Level-triggered: a change that already happened before the call is returned from the first
    /// level check, even if no future was ever waiting, so a value is never lost because nobody was
    /// listening. Cancel-safe: the wait runs on a private receiver clone and marks only that clone as
    /// observed, so dropping the returned future — before or after the change arrives — never
    /// consumes, advances or hides a version, and a later call with the same `seen` still observes
    /// it. Returns `None` only when the version channel is closed (no session sender remains), so a
    /// real close ends the wait instead of being masked as an endless stream.
    pub async fn changed_since(&self, seen: u64) -> Option<u64> {
        let mut waiter = self.changed.clone();
        loop {
            let current = waiter.borrow_and_update().version;
            if current != seen {
                return Some(current);
            }
            if waiter.changed().await.is_err() {
                return None;
            }
        }
    }

    /// Waits, outside every lock, until this consumer has a frame that must be delivered, and
    /// returns the version to take.
    ///
    /// `seen` is the consumer's own **observed** watermark — pass [`ChatUpdates::observed_version`],
    /// which advances for every change the consumer has looked past, not the delivered version. A
    /// delivered-only watermark would re-fire for a version the consumer already skipped and spin;
    /// the diff is always computed from the delivered baseline by [`ChatUpdates::take`].
    ///
    /// Readiness is decided from the two published numbers only — the window version and the
    /// boundary waterline — so the wait never builds a body, a snapshot or a diff:
    ///
    /// - A pending boundary (`urgent > seen`) is ready at once, so a first byte, an execution
    ///   terminal admission, a save acknowledgement, an authoritative replacement, a field removal
    ///   or a structural focus/window move is never delayed by text coalescing.
    /// - Ordinary already-started text (`version > seen`, `urgent <= seen`) holds **one** fixed
    ///   [`COALESCE_WINDOW`] deadline, anchored when the pending coalesced change is first observed
    ///   here. A later delta advances the version but never restarts that deadline, so a running
    ///   stream is delivered at a fixed cadence rather than pushed out token by token.
    /// - Nothing pending (`version == seen`) blocks until any change; a change that already happened
    ///   before the call is returned from the first level check (level-triggered, never edge-triggered).
    ///
    /// Cancel-safe like [`ChatWatch::changed_since`]: the wait runs on a private receiver clone, so
    /// dropping the future — before or after the change arrives — consumes no version and marks no
    /// shared state. Returns `None` only when the channel is closed (no session sender remains).
    ///
    /// This is the same readiness wait [`ChatUpdates::next`] composes with its one synchronous
    /// [`ChatUpdates::take`], so an HTTP `next` and a lock-outside/runtime `take` share one coalescing
    /// rule instead of each reinventing a timer.
    pub async fn wait(&self, seen: u64) -> Option<u64> {
        let mut waiter = self.changed.clone();
        // Anchored only once, on the first observation of a pending coalesced change, and never
        // restarted by a later delta.
        let mut deadline: Option<tokio::time::Instant> = None;
        loop {
            let progress = *waiter.borrow_and_update();
            if progress.version == seen {
                if waiter.changed().await.is_err() {
                    return None;
                }
                continue;
            }
            if progress.urgent > seen {
                return Some(progress.version);
            }
            let coalesce_deadline =
                *deadline.get_or_insert_with(|| tokio::time::Instant::now() + COALESCE_WINDOW);
            tokio::select! {
                biased;
                () = tokio::time::sleep_until(coalesce_deadline) => return Some(progress.version),
                changed = waiter.changed() => {
                    if changed.is_err() {
                        return None;
                    }
                }
            }
        }
    }
}

pub struct ChatUpdates {
    view: ChatView,
    watch: ChatWatch,
    /// The window content this consumer last **delivered** (the initial subscription, then every
    /// emitted frame's version). Diffs and `Patch::from` are always relative to this, so a consumer's
    /// continuity watermark only ever moves to a version it actually received. A change this window
    /// does not show leaves it untouched.
    baseline: ChatSnapshot,
    /// The highest version this consumer has already **observed** (woken for), whether that version
    /// produced a delivered frame or was skipped as invisible. It is the level-triggered wake
    /// watermark [`ChatWatch::wait`] is called with, so a skipped change does not make the wait
    /// re-fire for a version the consumer already looked past. It never substitutes for `baseline`.
    seen: u64,
}

impl ChatUpdates {
    /// The version of the window content this consumer has actually **delivered**.
    ///
    /// It is the initial subscription version and then the `to` of every frame [`ChatUpdates::take`]
    /// returned, so `Patch::from` always equals the consumer's own last received version. A change
    /// this window does not show never moves it: the consumer's continuity watermark only advances
    /// with content it received. Cheap and synchronous; reads only the version.
    pub fn delivered_version(&self) -> u64 {
        self.baseline.version
    }

    /// The highest version this consumer has already **observed** (the level-triggered wake
    /// watermark).
    ///
    /// This is the value to pass to [`ChatWatch::wait`] for a lock-outside wait: it advances for every
    /// change the consumer has looked past — a delivered frame or a change this window does not show
    /// — so the wait blocks on the next real change instead of re-firing on one already skipped. It is
    /// deliberately separate from [`ChatUpdates::delivered_version`], which stays at the last frame
    /// the consumer actually received. `take` computes its diff from the delivered baseline.
    pub fn observed_version(&self) -> u64 {
        self.seen
    }

    /// Takes the pending typed frame for *this* consumer in one synchronous critical section.
    ///
    /// There is no await point inside, so reading the window, diffing it against this consumer's
    /// delivered baseline and advancing the two watermarks are one atomic step: a caller may wait for
    /// readiness outside any lock and then call `take` briefly to compute the deliverable diff once.
    ///
    /// Two watermarks are kept apart on purpose. `seen` (observed) advances to the version just read
    /// for **every** new version, so a change this window does not show is looked past instead of
    /// re-firing the wait. `baseline` (delivered) advances **only** when a frame is emitted, so its
    /// version stays the consumer's own last received version and the next `Patch::from` is exactly
    /// that. A change with nothing visible to this window (only the session version moved, or an
    /// unrelated live update on a history window) therefore returns `None` without moving the
    /// delivered baseline, and the next real change is still diffed from the last frame the consumer
    /// received instead of from a version it never saw. A real change that cannot be expressed as a
    /// diff falls back to `Reset`, still advancing the delivered baseline with that same read.
    pub fn take(&mut self) -> Option<ChatUpdate> {
        let (update, next, _) = self.frame();
        if next.version == self.seen {
            // Nothing new has been observed since the last `take`: no frame, no watermark move.
            return None;
        }
        self.seen = next.version;
        // A `Reset` is always a real structural change. A `Patch` is deliverable when it carries a
        // field change, or when the new-content hint (`has_newer`) first flips so the host can show
        // it; an empty patch with an unchanged hint is an unrelated update this window does not show.
        let deliver = match &update {
            ChatUpdate::Reset(_) => true,
            ChatUpdate::Patch {
                changes, has_newer, ..
            } => !changes.is_empty() || *has_newer != self.baseline.has_newer,
        };
        if deliver {
            self.baseline = next;
            Some(update)
        } else {
            None
        }
    }

    /// Delivers the next frame by composing the lock-free readiness wait with one synchronous take.
    ///
    /// The wait is exactly [`ChatWatch::wait`]: a boundary frame (a first byte of a new identity, an
    /// execution terminal admission, a save acknowledgement, a field removal, an authoritative
    /// replacement or a structural focus/window move) is available at once, while ordinary
    /// already-started text is merged inside a single fixed window that a later delta never restarts.
    /// A suppressed change (see [`ChatUpdates::take`]) simply loops back to the wait, which blocks
    /// until the next real change instead of spinning.
    pub async fn next(&mut self) -> Option<ChatUpdate> {
        loop {
            // Wait on the *observed* watermark, never on the delivered one: a skipped change advances
            // only `seen`, so the wait blocks instead of re-firing for a version already looked past.
            self.watch.wait(self.seen).await?;
            if let Some(update) = self.take() {
                return Some(update);
            }
        }
    }

    /// Builds the frame against this consumer's baseline without committing it.
    ///
    /// The comparison keeps using the consumer's original baseline until the frame is committed, so a
    /// merged window never drops an intermediate append. The baseline is advanced by
    /// [`ChatUpdates::take`], which also decides whether the frame is visible to this window.
    fn frame(&self) -> (ChatUpdate, ChatSnapshot, ChatUpdatePriority) {
        let next = self.view.snapshot();
        if next.focus != self.baseline.focus
            || next.has_older != self.baseline.has_older
            || next.version < self.baseline.version
        {
            let update = ChatUpdate::Reset(next.clone());
            return (update, next, ChatUpdatePriority::Immediate);
        }
        let (changes, priority) = diff(&self.baseline.items, &next.items);
        let update = ChatUpdate::Patch {
            from: self.baseline.version,
            to: next.version,
            changes,
            has_newer: next.has_newer,
            priority,
        };
        (update, next, priority)
    }
}

impl ChatView {
    /// Current window version: cheap and synchronous, with no snapshot or diff.
    ///
    /// It is the same version domain as [`ChatSnapshot::version`] and as the value a [`ChatWatch`]
    /// wakes with, so a caller can compare it against a delivered baseline to decide whether to wait
    /// before entering a short critical section.
    pub fn version(&self) -> u64 {
        self.session.0.changed.borrow().version
    }

    /// A cheap, clonable, cancel-safe readiness handle over this window's progress fact.
    ///
    /// It is independent of any subscription: a caller can hold one clone for a lock-outside
    /// [`ChatWatch::wait`] while a short critical section computes exactly one diff with
    /// [`ChatUpdates::take`]. Every handle shares the session's version and boundary waterline.
    pub fn watch(&self) -> ChatWatch {
        ChatWatch {
            changed: self.session.0.changed.subscribe(),
        }
    }

    /// Reads the current frame and subscribes to later changes.
    ///
    /// The returned [`ChatUpdates`] is level-triggered from its baseline, so an update that lands
    /// between the snapshot and the subscription is still delivered by the next `next`/`take`
    /// instead of being silently skipped.
    pub fn subscribe(&self) -> (ChatSnapshot, ChatUpdates) {
        let initial = self.snapshot();
        (
            initial.clone(),
            ChatUpdates {
                view: self.clone(),
                watch: self.watch(),
                seen: initial.version,
                baseline: initial,
            },
        )
    }

    pub fn snapshot(&self) -> ChatSnapshot {
        let state = self
            .session
            .0
            .timeline
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let mut window = self
            .window
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if window.focus == ChatFocus::Latest {
            let pending = state
                .unsaved_order
                .values()
                .rev()
                .take(WINDOW_ITEMS)
                .filter_map(|id| resident_item(&state, id));
            // Resident in-flight items are shown with their complete content; a durable item keeps
            // its bounded preview until the host asks for the complete body by identity.
            let tail =
                state.recent.iter().rev().take(WINDOW_ITEMS).map(|item| {
                    resident_item(&state, &item.item_id).unwrap_or_else(|| item.clone())
                });
            let latest = merge_items(window.items.iter().cloned().chain(tail).chain(pending));
            let visible = window.items.len().clamp(INITIAL_ITEMS, WINDOW_ITEMS);
            window.items = latest.into_iter().rev().take(visible).collect();
            window.items.reverse();
            window.has_newer = false;
        } else if let Some(last) = window.items.last() {
            window.has_newer |= state
                .recent
                .back()
                .is_some_and(|item| item.order > last.order);
        }
        if let Some(first) = window.items.first() {
            window.has_older |= state.unsaved_order.range(..first.order).next().is_some()
                || state.recent.iter().any(|item| item.order < first.order);
        }
        if let Some(last) = window.items.last() {
            window.has_newer |= state
                .unsaved_order
                .range((
                    std::ops::Bound::Excluded(last.order),
                    std::ops::Bound::Unbounded,
                ))
                .next()
                .is_some();
        }
        overlay_complete(&mut window);
        window.version = state.version;
        ChatSnapshot {
            focus: window.focus.clone(),
            version: window.version,
            items: window.items.clone(),
            has_older: window.has_older,
            has_newer: window.has_newer,
        }
    }

    pub async fn focus(&self, focus: ChatFocus) -> Result<(), ChatError> {
        let generation = {
            let mut window = self
                .window
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            window.generation = window.generation.wrapping_add(1);
            window.generation
        };
        let (query, anchor) = match &focus {
            ChatFocus::Latest => (ChatQuery::Latest, None),
            ChatFocus::Around(id) => {
                let item = if let Some(item) = self.session.memory_preview_item(id) {
                    item
                } else {
                    let full = self
                        .session
                        .0
                        .history
                        .item(id)
                        .await?
                        .ok_or_else(|| ChatError::MissingAnchor(id.clone()))?;
                    self.session.0.history.bounded(&full)?
                };
                (ChatQuery::Around(item.order), Some(item))
            }
        };
        // A warm session can paint its first frame entirely from the shared tail.
        // Once fewer than a full first page is resident, SQLite must fill the gap.
        let memory = self.session.memory_page(query, RECENT_ITEMS);
        let warm_tail = focus == ChatFocus::Latest && memory.len() >= INITIAL_ITEMS;
        let page = if warm_tail {
            HistoryPage {
                items: Vec::new(),
                // The cache cannot establish that the first resident row is the
                // beginning of history. An older load will resolve the boundary.
                has_older: true,
                has_newer: false,
            }
        } else {
            self.session
                .bounded_page(self.session.0.history.page(query, RECENT_ITEMS).await?)?
        };
        if focus == ChatFocus::Latest && !warm_tail {
            self.session.hydrate(&page.items);
        }
        let mut items = merge_items(page.items.into_iter().chain(memory));
        let candidate_first = items.first().map(|item| item.order);
        let candidate_last = items.last().map(|item| item.order);
        if let Some(anchor) = anchor {
            let center = items
                .iter()
                .position(|item| item.item_id == anchor.item_id)
                .unwrap_or_else(|| {
                    items.push(anchor.clone());
                    items.sort_by_key(|item| item.order);
                    items
                        .iter()
                        .position(|item| item.item_id == anchor.item_id)
                        .unwrap_or(0)
                });
            let start = center.saturating_sub(INITIAL_ITEMS / 2);
            items = items.into_iter().skip(start).take(INITIAL_ITEMS).collect();
        } else {
            items = items.into_iter().rev().take(INITIAL_ITEMS).collect();
            items.reverse();
        }
        let has_older = page.has_older
            || matches!((candidate_first, items.first()), (Some(candidate), Some(first)) if candidate < first.order);
        let has_newer = page.has_newer
            || matches!((candidate_last, items.last()), (Some(candidate), Some(last)) if candidate > last.order);
        let mut window = self
            .window
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if window.generation != generation {
            return Ok(());
        }
        window.focus = focus;
        window.items = items;
        window.has_older = has_older;
        window.has_newer = has_newer;
        overlay_complete(&mut window);
        drop(window);
        self.session.notify();
        Ok(())
    }

    pub async fn load(&self, direction: Direction) -> Result<ChatSnapshot, ChatError> {
        let (anchor, focus, generation) = {
            let window = self
                .window
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let anchor = match direction {
                Direction::Older => window.items.first().map(|item| item.order),
                Direction::Newer => window.items.last().map(|item| item.order),
            };
            (anchor, window.focus.clone(), window.generation)
        };
        let Some(anchor) = anchor else {
            return Ok(self.snapshot());
        };
        let query = match direction {
            Direction::Older => ChatQuery::Before(anchor),
            Direction::Newer => ChatQuery::After(anchor),
        };
        let relevant = self.session.memory_page(query, PAGE_ITEMS);
        let page = self
            .session
            .bounded_page(self.session.0.history.page(query, PAGE_ITEMS).await?)?;
        let mut addition = merge_items(page.items.into_iter().chain(relevant));
        match direction {
            Direction::Older => addition = addition.into_iter().rev().take(PAGE_ITEMS).collect(),
            Direction::Newer => addition.truncate(PAGE_ITEMS),
        }
        let mut window = self
            .window
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if window.focus != focus
            || window.generation != generation
            || match direction {
                Direction::Older => window.items.first().map(|item| item.order),
                Direction::Newer => window.items.last().map(|item| item.order),
            } != Some(anchor)
        {
            drop(window);
            return Ok(self.snapshot());
        }
        let had_newer = window.has_newer;
        let had_older = window.has_older;
        if direction == Direction::Older
            && focus == ChatFocus::Latest
            && !addition.is_empty()
            && let Some(last) = window.items.last()
        {
            window.focus = ChatFocus::Around(last.item_id.clone());
        }
        window.items = merge_items(window.items.drain(..).chain(addition));
        let mut older_removed = false;
        let mut newer_removed = false;
        if window.items.len() > WINDOW_ITEMS {
            match direction {
                Direction::Older => {
                    window.items.truncate(WINDOW_ITEMS);
                    newer_removed = true;
                }
                Direction::Newer => {
                    let excess = window.items.len() - WINDOW_ITEMS;
                    window.items.drain(..excess);
                    older_removed = true;
                }
            }
        }
        window.has_older = if direction == Direction::Older {
            page.has_older || older_removed
        } else {
            had_older || older_removed
        };
        window.has_newer = if direction == Direction::Newer {
            page.has_newer || newer_removed
        } else {
            had_newer || newer_removed
        };
        overlay_complete(&mut window);
        drop(window);
        self.session.notify();
        Ok(self.snapshot())
    }

    /// Reads one canonical item from the in-memory tail or the durable history.
    pub async fn read_item(&self, item_id: &str) -> Result<Option<ChatItem>, ChatError> {
        self.session.read_item(item_id).await
    }

    /// Requests the complete body of one identity and returns it at this window's authoritative
    /// revision.
    ///
    /// In-flight content is read from memory; a durable item is read from storage at most once and
    /// then retained in this view until the identity leaves the window or the view is dropped, so a
    /// later frame does not reread the database. The request is a standing **complete-body intent for
    /// this view and identity**: once made, every later revision keeps following the complete shared
    /// fields at publication time, so the visible body never falls back to a bounded preview and no
    /// further storage read happens while the identity stays in the window. When the request changes
    /// the *visible* body of an item still in this window (for example a same-revision bounded
    /// preview upgraded to the complete body), the session's window version advances and watchers
    /// are notified, so a subscriber taking the current version sees the complete body instead of a
    /// version-equal no-op; a later producer update still arrives as a typed field change along this
    /// complete body. The item's `revision`
    /// is unchanged by the upgrade (`preview -> full` is a bounded-status change, not a content
    /// version change), and the complete body is independent of the save and execution watermarks: it
    /// neither sets `saved` nor `lifecycle`.
    ///
    /// The retained copy is bound to *this* view and only while the identity stays in this window, so
    /// a concurrent call or a call for an identity that left the window can neither associate the
    /// body with another window nor resurrect an abandoned identity, and it never overwrites a newer
    /// revision. Storage is never awaited while a timeline or window lock is held. Two callers that
    /// race on the same not-yet-retained durable identity may each read storage once before the first
    /// one fills the cache; every settled read is still a single read.
    pub async fn read_complete(&self, item_id: &str) -> Result<Option<ChatItem>, ChatError> {
        if let Some(item) = self.session.memory_item(item_id)
            && item.omitted_bytes == 0
        {
            return Ok(Some(self.retain_complete(item)));
        }
        // Take the cache hit in its own scope so the window guard is dropped **before** reconciling:
        // `reconcile_window_facts` locks the same non-reentrant mutex, and a scrutinee temporary
        // would otherwise stay alive across the `if let` body and self-deadlock.
        let cached = {
            let window = self
                .window
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            window.complete.get(item_id).cloned()
        };
        if let Some(cached) = cached {
            // The cache is a body snapshot: reconcile it with the window slot so a caller observes
            // the latest timeline-owned save/lifecycle facts rather than the ones captured when the
            // body was fetched.
            return Ok(Some(self.reconcile_window_facts(cached)));
        }
        let Some(full) = self.session.read_item(item_id).await? else {
            return Ok(None);
        };
        Ok(Some(self.retain_complete(full)))
    }

    /// Reconciles a cached body snapshot with the window slot at the same content revision.
    ///
    /// The row identity, `saved` save watermark and `lifecycle` execution fact are owned by the
    /// timeline and mirrored into the window slot by publication and [`Session::confirm_saved`]. A
    /// complete-content cache only supplies a body, so this never lets the cached body overwrite a
    /// newer save acknowledgement or execution terminal fact. Facts are copied **only** at the exact
    /// same revision: if the window has advanced past the cached body, the cached body is a stale
    /// read and the authoritative newest item is returned instead of a mixed-revision item; if the
    /// cached body is newer than the window slot it is returned on its own facts.
    fn reconcile_window_facts(&self, mut item: ChatItem) -> ChatItem {
        let window = self
            .window
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some(slot) = window
            .items
            .iter()
            .find(|candidate| candidate.item_id == item.item_id)
        else {
            return item;
        };
        match item.revision.cmp(&slot.revision) {
            Ordering::Equal => {
                item.saved = slot.saved;
                item.lifecycle = slot.lifecycle;
                item.turn_id = slot.turn_id.clone();
                item.meta = slot.meta.clone();
                item.order = slot.order;
                item
            }
            // The window advanced past the cached body: the cached body is a stale read.
            Ordering::Less => slot.clone(),
            // The cached body is newer than the window slot: keep it on its own facts.
            Ordering::Greater => item,
        }
    }

    /// Retains a complete item only while its identity stays in this view's window, and returns the
    /// effective body a caller should observe.
    ///
    /// When this actually upgrades the visible **body** of an item still in the window (for example a
    /// same-revision bounded preview upgraded to the complete body), it advances the session's window
    /// version and notifies watchers, so a subscriber taking the current version sees the complete
    /// body instead of a version-equal no-op. The row identity, `saved` watermark and `lifecycle` are
    /// copied **from** the window slot onto the retained body rather than the other way round: they
    /// are timeline-owned facts the cache must never roll back, so a `confirm_saved` or terminal
    /// admission that arrives at the same revision is not undone by a later overlay. Facts are only
    /// ever copied at the **same** content revision: a stale read for an identity the window already
    /// advanced past is discarded and the authoritative newest item is returned instead of a
    /// mixed-revision item; a body newer than the window slot is retained on its own facts. The cache
    /// is never overwritten by a stale revision. The window lock is released before the timeline is
    /// touched, preserving the timeline -> window lock order (no storage IO or timeline lock is ever
    /// taken while holding the window lock). An item that already left the window after a late read
    /// changes nothing, is neither retained nor notified.
    fn retain_complete(&self, item: ChatItem) -> ChatItem {
        if item.omitted_bytes != 0 {
            return item;
        }
        let mut effective = item;
        let mut visible_changed = false;
        let mut authoritative: Option<ChatItem> = None;
        {
            let mut window = self
                .window
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let Some(index) = window
                .items
                .iter()
                .position(|candidate| candidate.item_id == effective.item_id)
            else {
                return effective;
            };
            match effective.revision.cmp(&window.items[index].revision) {
                // The read is newer than the window slot: retain it on its own facts.
                Ordering::Greater => retain_cached_body(&mut window.complete, effective.clone()),
                // The window advanced past the read: the read is stale, discard it and return the
                // authoritative newest item instead of copying newer facts onto an older body.
                Ordering::Less => authoritative = Some(window.items[index].clone()),
                Ordering::Equal => {
                    // Keep the timeline-owned independent state facts, upgrade only the body.
                    let slot = &window.items[index];
                    effective.saved = slot.saved;
                    effective.lifecycle = slot.lifecycle;
                    effective.turn_id = slot.turn_id.clone();
                    effective.meta = slot.meta.clone();
                    effective.order = slot.order;
                    let before = (slot.revision, slot.omitted_bytes);
                    overlay_complete_body(&mut window.items[index], &effective);
                    let after = window.items[index].revision;
                    let after_omitted = window.items[index].omitted_bytes;
                    visible_changed = (after, after_omitted) != before;
                    retain_cached_body(&mut window.complete, effective.clone());
                }
            }
        }
        if let Some(authoritative) = authoritative {
            return authoritative;
        }
        if visible_changed {
            self.session.notify();
        }
        effective
    }
}

/// Stores a complete body in a window's cache, without letting a stale revision overwrite a newer one.
fn retain_cached_body(complete: &mut BTreeMap<String, ChatItem>, item: ChatItem) {
    let stale = complete
        .get(&item.item_id)
        .is_some_and(|existing| existing.revision > item.revision);
    if !stale {
        complete.insert(item.item_id.clone(), item);
    }
}

/// Applies the complete content this view requested and releases entries that left the window.
fn overlay_complete(window: &mut Window) {
    let Window {
        items, complete, ..
    } = window;
    for item in items.iter_mut() {
        if let Some(full) = complete.get(&item.item_id) {
            overlay_complete_body(item, full);
        }
    }
    complete.retain(|item_id, _| items.iter().any(|item| &item.item_id == item_id));
}

/// Upgrades only the visible **body** of `slot` from the complete-content cache, never the
/// independent state facts.
///
/// `saved`, `lifecycle`, `turn_id`, `meta`, `order` and the row identity are timeline-owned facts
/// already mirrored into the window slot by publication and [`Session::confirm_saved`]. The cache is
/// a body snapshot taken when the body was requested, so it may carry a stale save watermark or
/// execution lifecycle; overlaying the body must never roll those facts back. Only `fields` and the
/// bounded status are replaced, and only when the cached body is at the same content revision and
/// strictly more complete (`omitted_bytes` strictly smaller).
fn overlay_complete_body(slot: &mut ChatItem, full: &ChatItem) {
    if full.revision != slot.revision || full.omitted_bytes >= slot.omitted_bytes {
        return;
    }
    slot.fields = full.fields.clone();
    slot.omitted_bytes = full.omitted_bytes;
}

fn merge_items(items: impl IntoIterator<Item = ChatItem>) -> Vec<ChatItem> {
    let mut merged = BTreeMap::<String, ChatItem>::new();
    for item in items {
        let key = item.item_id.clone();
        // A newer revision wins outright. At the same revision the more complete body wins over a
        // bounded preview, but the independent state facts are reconciled monotonically (a save
        // acknowledgement only flips false -> true, an execution terminal fact only advances
        // streaming -> terminal): a more complete body must never let a stale copy roll back a
        // newer save watermark or execution terminal fact carried by the other candidate.
        if let Some(slot) = merged.get_mut(&key) {
            if slot.revision > item.revision {
                continue;
            }
            if slot.revision == item.revision {
                let saved = slot.saved || item.saved;
                let lifecycle = slot.lifecycle.max(item.lifecycle);
                if item.omitted_bytes < slot.omitted_bytes {
                    // Keep the more complete body, carry the monotonic facts forward.
                    let mut body = item;
                    body.saved = saved;
                    body.lifecycle = lifecycle;
                    *slot = body;
                } else {
                    slot.saved = saved;
                    slot.lifecycle = lifecycle;
                }
                continue;
            }
            // A strictly newer revision replaces the slot outright.
            *slot = item;
            continue;
        }
        merged.insert(key, item);
    }
    let mut items: Vec<_> = merged.into_values().collect();
    items.sort_by(|left, right| {
        left.order
            .cmp(&right.order)
            .then_with(|| left.item_id.cmp(&right.item_id))
    });
    items
}

/// Whether admitting `next` over `previous` is a frame boundary that must not wait for coalescing.
///
/// A boundary is a new identity (no resident predecessor), an execution-terminal admission, a save
/// receipt, a change to the static metadata or turn binding, or a field that was removed or
/// authoritatively replaced rather than appended. A pure append-only advance of an already-known
/// identity is ordinary already-started text and may be merged. The comparison is by shared block
/// lineage, so deciding the priority never materializes a body.
fn publication_is_boundary(previous: Option<&ChatItem>, next: &ChatItem) -> bool {
    let Some(previous) = previous else {
        return true;
    };
    if previous.item_id != next.item_id
        || previous.order != next.order
        || previous.lifecycle != next.lifecycle
        || previous.saved != next.saved
        || previous.turn_id != next.turn_id
        || previous.meta != next.meta
        || previous.fields.len() != next.fields.len()
    {
        return true;
    }
    next.fields.iter().any(|(field, block)| {
        let Some(previous_block) = previous.fields.get(field) else {
            return true;
        };
        !Arc::ptr_eq(previous_block, block)
            && ContentBlock::appended_since(block, previous_block).is_none()
    })
}

/// Whether two aligned items describe the same frame: same identity, watermark and bounded status.
///
/// Content is compared by shared block pointer, so a burst of appends does not materialize text.
fn same_frame(left: &ChatItem, right: &ChatItem) -> bool {
    left.item_id == right.item_id
        && left.order == right.order
        && left.revision == right.revision
        && left.lifecycle == right.lifecycle
        && left.saved == right.saved
        && left.turn_id == right.turn_id
        && left.meta == right.meta
        && left.omitted_bytes == right.omitted_bytes
        && left.fields.len() == right.fields.len()
        && left.fields.iter().all(|(field, block)| {
            right
                .fields
                .get(field)
                .is_some_and(|other| Arc::ptr_eq(block, other))
        })
}

/// Whether two aligned items keep the same row identity: identity, order, terminal lifecycle and
/// static metadata. Such a row may be updated in place; anything else is a structural splice.
fn same_row(left: &ChatItem, right: &ChatItem) -> bool {
    left.item_id == right.item_id
        && left.order == right.order
        && left.lifecycle == right.lifecycle
        && left.turn_id == right.turn_id
        && left.meta == right.meta
}

/// Batched content change for one item that kept its row identity, or `None` when nothing changed.
///
/// Every field delta of the identity travels under one `expected_revision` / `revision` commit, and
/// a version-only or bounded-status-only change still produces a (field-delta-free) update so a
/// consumer never keeps a stale revision or omitted status. The returned flag marks a boundary: the
/// execution lifecycle or save watermark changed, or a field was removed or authoritatively
/// replaced rather than appended.
fn item_update(previous: &ChatItem, next: &ChatItem) -> Option<(ViewChange, bool)> {
    if same_frame(previous, next) {
        return None;
    }
    let mut fields = Vec::new();
    for (field, block) in &next.fields {
        let change = match previous.fields.get(field) {
            Some(previous_block) if Arc::ptr_eq(previous_block, block) => FieldChange::Unchanged,
            Some(previous_block) => match ContentBlock::appended_since(block, previous_block) {
                Some(text) if text.is_empty() => FieldChange::Unchanged,
                Some(text) => FieldChange::Append(text),
                None => FieldChange::Replace(block.clone()),
            },
            None => FieldChange::Replace(block.clone()),
        };
        fields.push(FieldUpdate {
            field: field.clone(),
            change,
        });
    }
    // A field present before but gone now is an explicit removal, not a silently dropped delta.
    for field in previous.fields.keys() {
        if !next.fields.contains_key(field) {
            fields.push(FieldUpdate {
                field: field.clone(),
                change: FieldChange::Remove,
            });
        }
    }
    let boundary = previous.lifecycle != next.lifecycle
        || previous.saved != next.saved
        || fields
            .iter()
            .any(|delta| matches!(&delta.change, FieldChange::Remove | FieldChange::Replace(_)));
    let change = ViewChange::UpdateItem {
        item_id: next.item_id.clone(),
        expected_revision: previous.revision,
        revision: next.revision,
        omitted_bytes: next.omitted_bytes,
        saved: next.saved,
        fields,
    };
    Some((change, boundary))
}

/// Diffs two windows into typed changes together with the frame's priority fact.
///
/// Boundaries — a structural splice, an execution terminal admission, a save acknowledgement, an
/// authoritative replacement or a field removal — are delivered immediately. Only ordinary
/// already-started text (appends, or re-versioned fields that are otherwise unchanged) coalesces.
fn diff(previous: &[ChatItem], next: &[ChatItem]) -> (Vec<ViewChange>, ChatUpdatePriority) {
    // A window of the same length usually changed only in place, so emit one batched item update per
    // advanced identity instead of a whole splice.
    if previous.len() == next.len()
        && previous
            .iter()
            .zip(next)
            .all(|(old, new)| same_row(old, new))
    {
        let mut changes = Vec::new();
        let mut boundary = false;
        for (old, new) in previous.iter().zip(next) {
            if let Some((change, is_boundary)) = item_update(old, new) {
                boundary |= is_boundary;
                changes.push(change);
            }
        }
        let priority = if boundary {
            ChatUpdatePriority::Immediate
        } else {
            ChatUpdatePriority::Coalesced
        };
        return (changes, priority);
    }
    let prefix = previous
        .iter()
        .zip(next)
        .take_while(|(left, right)| same_frame(left, right))
        .count();
    let suffix = previous[prefix..]
        .iter()
        .rev()
        .zip(next[prefix..].iter().rev())
        .take_while(|(left, right)| same_frame(left, right))
        .count();
    (
        vec![ViewChange::Splice {
            index: prefix,
            remove: previous.len() - prefix - suffix,
            items: next[prefix..next.len() - suffix].to_vec(),
        }],
        ChatUpdatePriority::Immediate,
    )
}
