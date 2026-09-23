//! Bounded, storage-independent reading windows over one session's timeline.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fmt,
    future::Future,
    sync::{Arc, Mutex, Weak},
};

use futures::future::BoxFuture;
use tokio::sync::watch;

const RECENT_ITEMS: usize = 100;
const INITIAL_ITEMS: usize = 32;
const WINDOW_ITEMS: usize = 96;
const PAGE_ITEMS: usize = 32;
const RECENT_BYTES: usize = 8 * 1024 * 1024;
const ITEM_PREVIEW_BYTES: usize = 256 * 1024;
const ACTIVE_PREVIEW_BYTES: usize = 16 * 1024 * 1024;

/// A provider part's identity does not depend on late output indexes or part IDs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationPart {
    OutputText(u32),
    ReasoningText(u32),
    SummaryText(u32),
}

pub fn presentation_prefix(attempt_id: &str) -> String {
    format!("model:{}:{attempt_id}:presentation:item:", attempt_id.len())
}

pub fn presentation_item_id(
    attempt_id: &str,
    provider_item_id: &str,
    part: Option<PresentationPart>,
) -> String {
    let base = format!(
        "{}{}:{provider_item_id}",
        presentation_prefix(attempt_id),
        provider_item_id.len(),
    );
    match part {
        Some(PresentationPart::OutputText(index)) => format!("{base}:text:{index}"),
        Some(PresentationPart::ReasoningText(index)) => format!("{base}:reasoning:{index}"),
        Some(PresentationPart::SummaryText(index)) => format!("{base}:summary:{index}"),
        None => format!("{base}:empty"),
    }
}

/// A presentation item; payload belongs to the host's immutable presentation codec.
/// The model context is unrelated to this representation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatItem {
    pub item_id: String,
    pub turn_id: String,
    pub order: u64,
    pub revision: u64,
    pub part_id: Option<String>,
    pub body: Arc<str>,
    /// Bytes omitted from this presentation body; zero means the body is complete.
    pub omitted_bytes: u64,
    pub saved: bool,
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
    /// Project a bounded visible copy without changing the canonical item. Codec-aware
    /// hosts should override this to keep their presentation body decodable.
    fn preview(&self, item: &ChatItem) -> Result<ChatItem, ChatError> {
        if item.body.len() <= ITEM_PREVIEW_BYTES {
            return Ok(item.clone());
        }
        let mut end = ITEM_PREVIEW_BYTES;
        while !item.body.is_char_boundary(end) {
            end -= 1;
        }
        let mut preview = item.clone();
        preview.body = Arc::from(&item.body[..end]);
        preview.omitted_bytes = item
            .omitted_bytes
            .saturating_add((item.body.len() - end) as u64);
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

    fn item(
        &self,
        item_id: &str,
    ) -> impl Future<Output = Result<Option<ChatItem>, ChatError>> + Send;

    fn read_body(
        &self,
        item_id: &str,
    ) -> impl Future<Output = Result<Option<Arc<str>>, ChatError>> + Send;
}

trait ErasedChatHistory: Send + Sync + fmt::Debug {
    fn preview(&self, item: &ChatItem) -> Result<ChatItem, ChatError>;
    fn latest_allocated_order(&self) -> BoxFuture<'_, Result<u64, ChatError>>;
    fn page(&self, query: ChatQuery, limit: usize)
    -> BoxFuture<'_, Result<HistoryPage, ChatError>>;
    fn item<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<Option<ChatItem>, ChatError>>;
    fn read_body<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<Option<Arc<str>>, ChatError>>;
}

impl<T: ChatHistory> ErasedChatHistory for T {
    fn preview(&self, item: &ChatItem) -> Result<ChatItem, ChatError> {
        ChatHistory::preview(self, item)
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

    fn read_body<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<Option<Arc<str>>, ChatError>> {
        Box::pin(ChatHistory::read_body(self, id))
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
    // are not reliable history and have a separate, strictly bounded memory budget.
    active_previews: BTreeMap<String, ChatItem>,
    active_preview_bytes: usize,
    allocated: BTreeMap<String, u64>,
    allocated_order: BTreeMap<u64, String>,
    highest_order: u64,
    version: u64,
}

struct SessionInner {
    history: Arc<dyn ErasedChatHistory>,
    order_seed: tokio::sync::OnceCell<u64>,
    timeline: Mutex<TimelineState>,
    windows: Mutex<Vec<Weak<Mutex<Window>>>>,
    changed: watch::Sender<u64>,
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
        let (changed, _) = watch::channel(0);
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

    fn drop_previews_matching(&self, matches: impl Fn(&str) -> bool) {
        let mut state = self
            .0
            .timeline
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let mut abandoned: BTreeSet<String> = state
            .allocated
            .keys()
            .filter(|id| matches(id) && !state.unsaved.contains_key(*id))
            .cloned()
            .collect();
        abandoned.extend(
            state
                .recent
                .iter()
                .filter(|item| {
                    matches(&item.item_id)
                        && !item.saved
                        && !state.unsaved.contains_key(&item.item_id)
                })
                .map(|item| item.item_id.clone()),
        );
        if abandoned.is_empty() {
            return;
        }
        for id in &abandoned {
            if let Some(order) = state.allocated.remove(id) {
                state.allocated_order.remove(&order);
            }
            if let Some(old) = state.active_previews.remove(id) {
                state.active_preview_bytes -= old.body.len();
            }
        }
        let old_len = state.recent.len();
        state
            .recent
            .retain(|item| !abandoned.contains(&item.item_id));
        state.recent_bytes = state.recent.iter().map(|item| item.body.len()).sum();
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
            state.version = state.version.wrapping_add(1);
            self.0.changed.send_replace(state.version);
        }
    }

    fn publish_inner(&self, item: ChatItem, reliable: bool) -> Result<(), ChatError> {
        if reliable && !item.saved && item.omitted_bytes != 0 {
            return Err(ChatError::Conflict(item.item_id));
        }
        let visible = self.0.history.preview(&item)?;
        let mut state = self
            .0
            .timeline
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if item.order == 0 || item.order > i64::MAX as u64 {
            return Err(ChatError::Conflict(item.item_id));
        }
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
        if let Some(previous) = state
            .unsaved
            .get(&item.item_id)
            .or_else(|| state.recent.iter().find(|old| old.item_id == item.item_id))
        {
            if previous.order != item.order
                || previous.part_id != item.part_id
                || (!previous.turn_id.is_empty()
                    && previous.turn_id != item.turn_id
                    && !(item.saved && !previous.saved))
            {
                return Err(ChatError::Conflict(item.item_id));
            }
            if previous.revision > item.revision {
                return Ok(());
            }
            if previous.revision == item.revision {
                if ((previous.omitted_bytes == 0
                    && item.omitted_bytes == 0
                    && previous.body != item.body)
                    || previous.turn_id != item.turn_id)
                    && (reliable || previous.saved || state.unsaved.contains_key(&item.item_id))
                    && !(item.saved && !previous.saved)
                {
                    return Err(ChatError::Conflict(item.item_id));
                }
                if previous.saved && !item.saved {
                    return Ok(());
                }
            }
        }
        state.highest_order = state.highest_order.max(item.order);
        if let Some(old) = state.active_previews.remove(&item.item_id) {
            state.active_preview_bytes -= old.body.len();
        }
        if !reliable && !item.saved && visible.omitted_bytes != 0 {
            if item.body.len() <= ACTIVE_PREVIEW_BYTES {
                state.active_preview_bytes += item.body.len();
                state
                    .active_previews
                    .insert(item.item_id.clone(), item.clone());
            }
            while state.active_preview_bytes > ACTIVE_PREVIEW_BYTES {
                let Some(oldest) = state
                    .active_previews
                    .values()
                    .min_by_key(|entry| entry.order)
                    .map(|entry| entry.item_id.clone())
                else {
                    break;
                };
                if let Some(old) = state.active_previews.remove(&oldest) {
                    state.active_preview_bytes -= old.body.len();
                }
            }
        }
        if reliable || item.saved {
            state.allocated.remove(&item.item_id);
            state.allocated_order.remove(&item.order);
        }
        state.recent.retain(|old| old.item_id != item.item_id);
        state.recent_bytes = state.recent.iter().map(|old| old.body.len()).sum();
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
        state.recent_bytes = state.recent_bytes.saturating_add(visible.body.len());
        state.recent.push_back(visible.clone());
        state
            .recent
            .make_contiguous()
            .sort_by_key(|item| item.order);
        while state.recent.len() > RECENT_ITEMS || state.recent_bytes > RECENT_BYTES {
            let Some(old) = state.recent.pop_front() else {
                break;
            };
            state.recent_bytes -= old.body.len();
        }
        self.update_visible(&item.item_id, |previous| {
            if previous.revision <= item.revision {
                *previous = visible.clone();
            }
        });
        state.version = state.version.wrapping_add(1);
        self.0.changed.send_replace(state.version);
        Ok(())
    }

    /// An older writer acknowledgement cannot mark a newer revision as durable.
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
            .find(|item| item.item_id == item_id && item.revision == revision)
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
            state.version = state.version.wrapping_add(1);
            self.0.changed.send_replace(state.version);
        }
    }

    pub async fn open_chat(&self, focus: ChatFocus) -> Result<ChatView, ChatError> {
        let view = ChatView {
            session: self.clone(),
            window: Arc::new(Mutex::new(Window {
                focus: ChatFocus::Latest,
                items: Vec::new(),
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
        let unsaved = |order: &String| state.unsaved_previews.get(order).cloned();
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

    /// Reads the full body independently of the bounded presentation window.
    pub async fn read_body(&self, item_id: &str) -> Result<Option<Arc<str>>, ChatError> {
        if let Some(item) = self.memory_item(item_id) {
            if item.omitted_bytes == 0 {
                return Ok(Some(item.body));
            }
            return self
                .0
                .history
                .read_body(item_id)
                .await?
                .map(Some)
                .ok_or_else(|| {
                    ChatError::History(
                        std::io::Error::other(format!(
                            "full chat body for {item_id} is not available yet"
                        ))
                        .into(),
                    )
                });
        }
        self.0.history.read_body(item_id).await
    }

    fn memory_preview_item(&self, item_id: &str) -> Option<ChatItem> {
        let state = self
            .0
            .timeline
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state
            .unsaved_previews
            .get(item_id)
            .or_else(|| state.recent.iter().find(|item| item.item_id == item_id))
            .cloned()
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
            state.recent_bytes = state.recent_bytes.saturating_add(item.body.len());
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
            state.recent_bytes -= old.body.len();
        }
    }

    fn bounded_page(&self, mut page: HistoryPage) -> Result<HistoryPage, ChatError> {
        for item in &mut page.items {
            *item = self.0.history.preview(item)?;
        }
        Ok(page)
    }

    fn notify(&self) {
        let mut state = self
            .0
            .timeline
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state.version = state.version.wrapping_add(1);
        self.0.changed.send_replace(state.version);
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

struct Window {
    focus: ChatFocus,
    items: Vec<ChatItem>,
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
    pub version: u64,
    pub items: Vec<ChatItem>,
    pub has_older: bool,
    pub has_newer: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewChange {
    Splice {
        index: usize,
        remove: usize,
        items: Vec<ChatItem>,
    },
    AppendText {
        item_id: String,
        part_id: String,
        expected_revision: u64,
        revision: u64,
        text: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatUpdate {
    Reset(ChatSnapshot),
    Patch {
        from: u64,
        to: u64,
        changes: Vec<ViewChange>,
        has_newer: bool,
    },
}

pub struct ChatUpdates {
    view: ChatView,
    changed: watch::Receiver<u64>,
    baseline: ChatSnapshot,
}

impl ChatUpdates {
    pub async fn next(&mut self) -> Option<ChatUpdate> {
        self.changed.changed().await.ok()?;
        self.changed.borrow_and_update();
        if !self.baseline.items.is_empty() {
            // A slow reader already catches up through watch. Keep continuous
            // content updates below display rate without delaying the first item.
            tokio::time::sleep(std::time::Duration::from_millis(33)).await;
            self.changed.borrow_and_update();
        }
        let next = self.view.snapshot();
        let update = if next.version == self.baseline.version.wrapping_add(1)
            && next.focus == self.baseline.focus
            && next.has_older == self.baseline.has_older
        {
            ChatUpdate::Patch {
                from: self.baseline.version,
                to: next.version,
                changes: diff(&self.baseline.items, &next.items),
                has_newer: next.has_newer,
            }
        } else {
            ChatUpdate::Reset(next.clone())
        };
        self.baseline = next;
        Some(update)
    }
}

impl ChatView {
    /// Subscribes before reading the first frame so a concurrent update is either
    /// present in that frame or delivered by the receiver, never silently skipped.
    pub fn subscribe(&self) -> (ChatSnapshot, ChatUpdates) {
        let mut changed = self.session.0.changed.subscribe();
        changed.borrow_and_update();
        let initial = self.snapshot();
        (
            initial.clone(),
            ChatUpdates {
                view: self.clone(),
                changed,
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
                .filter_map(|id| state.unsaved_previews.get(id))
                .cloned();
            let latest = merge_items(
                window
                    .items
                    .iter()
                    .cloned()
                    .chain(state.recent.iter().rev().take(WINDOW_ITEMS).cloned())
                    .chain(pending),
            );
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
                    self.session.0.history.preview(&full)?
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
        drop(window);
        self.session.notify();
        Ok(self.snapshot())
    }

    pub async fn read_body(&self, item_id: &str) -> Result<Option<Arc<str>>, ChatError> {
        self.session.read_body(item_id).await
    }

    /// Reads one canonical item from the in-memory tail or the durable history.
    pub async fn read_item(&self, item_id: &str) -> Result<Option<ChatItem>, ChatError> {
        self.session.read_item(item_id).await
    }
}

fn merge_items(items: impl IntoIterator<Item = ChatItem>) -> Vec<ChatItem> {
    let mut merged = BTreeMap::<String, ChatItem>::new();
    for item in items {
        let key = item.item_id.clone();
        if merged
            .get(&key)
            .is_none_or(|old| old.revision <= item.revision)
        {
            merged.insert(key, item);
        }
    }
    let mut items: Vec<_> = merged.into_values().collect();
    items.sort_by(|left, right| {
        left.order
            .cmp(&right.order)
            .then_with(|| left.item_id.cmp(&right.item_id))
    });
    items
}

fn diff(previous: &[ChatItem], next: &[ChatItem]) -> Vec<ViewChange> {
    if previous == next {
        return Vec::new();
    }
    let prefix = previous
        .iter()
        .zip(next)
        .take_while(|(left, right)| left == right)
        .count();
    if previous.len() == next.len()
        && prefix + 1 == previous.len()
        && let (Some(old), Some(new)) = (previous.last(), next.last())
        && old.item_id == new.item_id
        && old.part_id == new.part_id
        && old.saved == new.saved
        && old.omitted_bytes == 0
        && new.omitted_bytes == 0
        && let Some(part_id) = &old.part_id
        && new.revision > old.revision
        && new.body.starts_with(old.body.as_ref())
    {
        return vec![ViewChange::AppendText {
            item_id: new.item_id.clone(),
            part_id: part_id.clone(),
            expected_revision: old.revision,
            revision: new.revision,
            text: new.body[old.body.len()..].to_owned(),
        }];
    }
    let suffix = previous[prefix..]
        .iter()
        .rev()
        .zip(next[prefix..].iter().rev())
        .take_while(|(left, right)| left == right)
        .count();
    vec![ViewChange::Splice {
        index: prefix,
        remove: previous.len() - prefix - suffix,
        items: next[prefix..next.len() - suffix].to_vec(),
    }]
}
