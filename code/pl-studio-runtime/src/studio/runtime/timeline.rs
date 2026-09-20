//! Rebuildable item index owned by Thread residency.
//!
//! Cold history pages are served from the durable Studio timeline index that lives beside the
//! canonical Thread journal inside the same per-Thread session database; a resident owner overlays
//! the same slot/watermark identity from memory. Neither path activates a Thread owner, replays a
//! whole journal, or creates a model or tool. A missing or uninitialised index is a typed
//! preparing/failure result driven by an explicit, cancellable, per-Thread index worker
//! (design/15 §15.5, design/17 §17.2, design/18 §18.3).
use super::StudioRuntime;
use crate::studio::timeline_store::{
    DEFAULT_PAGE_BYTES, TimelineBudget, TimelineEntry, TimelinePageQuery, TimelineReader,
    TimelineStoreError, TimelineWindow, build_index,
};
use anyhow::{Result, bail};
use base64::Engine as _;
use pl_core::thread::journal::ThreadCommit;
use pl_protocol::{ThreadItem, TimelinePage, TimelineQuery, TimelineTurn};
use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex as AsyncMutex;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

/// Bounded journal page size when an index worker folds the canonical journal.
const INDEX_BUILD_PAGE: usize = 256;
/// Maximum number of cached read-only index connections held by residency.
const READER_CACHE_CAPACITY: usize = 4;
/// Bounded slice size when reassembling an oversized item's display content.
const CONTENT_READ_BYTES: usize = 64 * 1024;

#[derive(Default)]
pub(super) struct TimelineIndex {
    watermark: u64,
    items: BTreeMap<u64, ThreadItem>,
    positions: BTreeMap<String, u64>,
    turns: Vec<TimelineTurn>,
}

impl TimelineIndex {
    pub(super) fn watermark(&self) -> u64 {
        self.watermark
    }

    pub(super) fn update(
        &mut self,
        watermark: u64,
        items: &[ThreadItem],
        turns: Vec<TimelineTurn>,
    ) {
        if watermark < self.watermark {
            return;
        }
        // Items are immutable identities. A new projection updates admitted/changed
        // entries only; absence in a preview is not a deletion.
        for item in items {
            // Ephemeral preview order is subscription-local. History pages do
            // not arbitrate competing previews at the same committed watermark.
            if watermark == self.watermark && self.items.contains_key(&item.ordinal) {
                continue;
            }
            if self.items.get(&item.ordinal) != Some(item) {
                self.positions.insert(item.id.clone(), item.ordinal);
                self.items.insert(item.ordinal, item.clone());
            }
        }
        self.watermark = watermark;
        self.turns = turns;
    }

    fn page(&self, thread_id: &str, query: &TimelineQuery, limit: usize) -> Result<TimelinePage> {
        let limit = limit.clamp(1, 100);
        let cursor = match query {
            TimelineQuery::Latest => None,
            TimelineQuery::Before { item_id } | TimelineQuery::After { item_id } | TimelineQuery::Around { item_id } => {
                Some(*self.positions.get(item_id).ok_or_else(|| anyhow::anyhow!("timeline query.itemId does not belong to this Thread; reload latest or choose an available item"))?)
            }
        };
        let items: Vec<_> = match (query, cursor) {
            (TimelineQuery::Latest, _) => self
                .items
                .values()
                .rev()
                .take(limit)
                .rev()
                .cloned()
                .collect(),
            (TimelineQuery::Before { .. }, Some(position)) => {
                let mut items: Vec<_> = self
                    .items
                    .range(..position)
                    .rev()
                    .take(limit)
                    .map(|(_, item)| item.clone())
                    .collect();
                items.reverse();
                items
            }
            (TimelineQuery::After { .. }, Some(position)) => self
                .items
                .range((
                    std::ops::Bound::Excluded(position),
                    std::ops::Bound::Unbounded,
                ))
                .take(limit)
                .map(|(_, item)| item.clone())
                .collect(),
            (TimelineQuery::Around { .. }, Some(position)) => {
                let start = self
                    .items
                    .range(..position)
                    .rev()
                    .take(limit / 2)
                    .last()
                    .map_or(position, |(ordinal, _)| *ordinal);
                self.items
                    .range(start..)
                    .take(limit)
                    .map(|(_, item)| item.clone())
                    .collect()
            }
            _ => bail!("timeline query has no cursor"),
        };
        let ids: BTreeSet<_> = items.iter().map(|item| item.turn_id.as_str()).collect();
        Ok(TimelinePage {
            thread_id: thread_id.into(),
            watermark: self.watermark,
            older_cursor: items
                .first()
                .filter(|item| self.items.range(..item.ordinal).next().is_some())
                .map(|item| item.id.clone()),
            newer_cursor: items
                .last()
                .filter(|item| {
                    self.items
                        .range((
                            std::ops::Bound::Excluded(item.ordinal),
                            std::ops::Bound::Unbounded,
                        ))
                        .next()
                        .is_some()
                })
                .map(|item| item.id.clone()),
            first_item_id: items.first().map(|item| item.id.clone()),
            last_item_id: items.last().map(|item| item.id.clone()),
            turns: self
                .turns
                .iter()
                .filter(|entry| ids.contains(entry.turn.id.as_str()))
                .cloned()
                .collect(),
            items,
        })
    }
}

/// Typed reason a cold Timeline read could not be served from the durable index.
///
/// All three variants are surfaced as the anyhow error of [`StudioRuntime::list_timeline_items`],
/// so a caller (and the later wire adapter) can distinguish "retry after preparation" from an
/// explicit failure. An empty page is never substituted for any of them.
#[derive(Debug, thiserror::Error)]
pub(crate) enum TimelineUnavailable {
    #[error(
        "Thread {thread_id} timeline index is being prepared; retry after preparation completes"
    )]
    Preparing { thread_id: String },
    #[error("Thread {thread_id} timeline index preparation failed: {source}")]
    Failed {
        thread_id: String,
        source: Arc<anyhow::Error>,
    },
    #[error("Thread {thread_id} timeline index is unavailable: {source}")]
    Unavailable {
        thread_id: String,
        source: TimelineStoreError,
    },
    #[error("Thread {thread_id} has no durable timeline index in this runtime")]
    NotDurable { thread_id: String },
}

/// Progress of one Thread's index worker; ready and failed states are retained for diagnostics.
#[derive(Debug, Clone)]
enum IndexState {
    Preparing,
    Ready,
    Failed(Arc<anyhow::Error>),
}

/// Bounded residency-owned cold index access: read-only connections plus one-shot build workers.
#[derive(Clone)]
pub(super) struct TimelineAccess {
    inner: Arc<TimelineAccessInner>,
}

struct TimelineAccessInner {
    readers: AsyncMutex<ReaderCache>,
    workers: AsyncMutex<BTreeMap<String, IndexWorker>>,
}

struct ReaderCache {
    capacity: usize,
    clock: u64,
    entries: BTreeMap<String, CachedReader>,
}

struct CachedReader {
    reader: Arc<TimelineReader>,
    used: u64,
}

struct IndexWorker {
    cancel: CancellationToken,
    state: watch::Receiver<IndexState>,
    task: tokio::task::JoinHandle<()>,
}

impl TimelineAccess {
    pub(super) fn new() -> Self {
        Self {
            inner: Arc::new(TimelineAccessInner {
                readers: AsyncMutex::new(ReaderCache {
                    capacity: READER_CACHE_CAPACITY,
                    clock: 0,
                    entries: BTreeMap::new(),
                }),
                workers: AsyncMutex::new(BTreeMap::new()),
            }),
        }
    }

    /// Returns a cached read-only index reader, opening and evicting within the bounded capacity.
    ///
    /// # Errors
    /// Propagates the typed open failure (missing database, uninitialised or unsupported index).
    pub(super) async fn reader(
        &self,
        thread_id: &str,
        path: &Path,
    ) -> Result<Arc<TimelineReader>, TimelineStoreError> {
        let mut cache = self.inner.readers.lock().await;
        cache.clock += 1;
        let clock = cache.clock;
        if let Some(entry) = cache.entries.get_mut(thread_id) {
            entry.used = clock;
            return Ok(entry.reader.clone());
        }
        let reader = Arc::new(TimelineReader::open(path).await?);
        while cache.entries.len() >= cache.capacity {
            let victim = cache
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(id, _)| id.clone());
            match victim {
                Some(id) => {
                    if let Some(entry) = cache.entries.remove(&id) {
                        close_reader(entry.reader).await;
                    }
                }
                None => break,
            }
        }
        cache.entries.insert(
            thread_id.to_owned(),
            CachedReader {
                reader: reader.clone(),
                used: clock,
            },
        );
        Ok(reader)
    }

    /// Releases one Thread's cached reader and stops its pending index worker.
    pub(super) async fn release(&self, thread_id: &str) {
        if let Some(entry) = self.inner.readers.lock().await.entries.remove(thread_id) {
            close_reader(entry.reader).await;
        }
        if let Some(worker) = self.inner.workers.lock().await.remove(thread_id) {
            worker.cancel.cancel();
            worker.task.abort();
        }
    }

    /// Stops every cached reader and pending worker; used when the runtime stops accepting work.
    pub(super) async fn stop_all(&self) {
        let readers = {
            let mut cache = self.inner.readers.lock().await;
            std::mem::take(&mut cache.entries)
        };
        for entry in readers.into_values() {
            close_reader(entry.reader).await;
        }
        let workers = {
            let mut guard = self.inner.workers.lock().await;
            std::mem::take(&mut *guard)
        };
        for worker in workers.into_values() {
            worker.cancel.cancel();
            worker.task.abort();
        }
    }

    /// Current worker state for a Thread, if any worker was started.
    async fn worker_state(&self, thread_id: &str) -> Option<IndexState> {
        let workers = self.inner.workers.lock().await;
        workers.get(thread_id).map(|worker| {
            if worker.task.is_finished() {
                worker.state.borrow().clone()
            } else {
                IndexState::Preparing
            }
        })
    }

    /// Starts one Thread's index worker unless an unfinished worker is already running.
    async fn kick(&self, path: PathBuf, thread_id: &str, parent_id: Option<String>) -> IndexState {
        let mut workers = self.inner.workers.lock().await;
        if let Some(worker) = workers.get(thread_id)
            && !worker.task.is_finished()
        {
            return IndexState::Preparing;
        }
        let (sender, receiver) = watch::channel(IndexState::Preparing);
        let cancel = CancellationToken::new();
        let worker_cancel = cancel.clone();
        let worker_thread = thread_id.to_owned();
        let task = tokio::spawn(async move {
            let state = run_index_build(path, worker_thread, parent_id, worker_cancel).await;
            sender.send_replace(state);
        });
        workers.insert(
            thread_id.to_owned(),
            IndexWorker {
                cancel,
                state: receiver,
                task,
            },
        );
        IndexState::Preparing
    }
}

async fn close_reader(reader: Arc<TimelineReader>) {
    if let Ok(reader) = Arc::try_unwrap(reader) {
        let _ = reader.close().await;
    }
}

/// Reads the whole canonical journal through bounded, non-hydrating pages and builds the index.
///
/// The core journal reader is read-only, starts no writer and hydrates no resource map, so the
/// worker never activates a Thread and never creates a model or tool. The derived index driver
/// still validates and folds one contiguous journal slice; its per-commit application is bounded.
async fn run_index_build(
    path: PathBuf,
    thread_id: String,
    parent_id: Option<String>,
    cancel: CancellationToken,
) -> IndexState {
    let outcome = build_index_from_journal(&path, &thread_id, parent_id.as_deref(), &cancel).await;
    match outcome {
        Ok(()) => IndexState::Ready,
        Err(error) => IndexState::Failed(Arc::new(error)),
    }
}

async fn build_index_from_journal(
    path: &Path,
    thread_id: &str,
    parent_id: Option<&str>,
    cancel: &CancellationToken,
) -> Result<()> {
    let reader =
        pl_core::persistence::open_journal_reader(pl_core::persistence::SqliteSessionOptions {
            path: path.to_path_buf(),
        })
        .await
        .map_err(|error| anyhow::anyhow!("open Thread {thread_id} journal reader: {error}"))?;
    let limit = NonZeroUsize::new(INDEX_BUILD_PAGE).expect("constant is nonzero");
    let mut commits = Vec::<Arc<ThreadCommit>>::new();
    let mut after = 0u64;
    loop {
        if cancel.is_cancelled() {
            bail!("Thread {thread_id} timeline index build was cancelled");
        }
        let page = reader
            .read_page(thread_id, after, limit)
            .await
            .map_err(|error| anyhow::anyhow!("read Thread {thread_id} journal: {error}"))?;
        if page.is_empty() {
            break;
        }
        after = page.last().map(|commit| commit.sequence).unwrap_or(after);
        commits.extend(page);
    }
    reader
        .close()
        .await
        .map_err(|error| anyhow::anyhow!("close Thread {thread_id} journal reader: {error}"))?;
    build_index(path, thread_id, parent_id, &commits)
        .await
        .map_err(|error| anyhow::anyhow!("build Thread {thread_id} timeline index: {error}"))?;
    Ok(())
}

/// True when an index fault is repaired by building the Thread's index rather than failing.
fn is_preparable(error: &TimelineStoreError) -> bool {
    matches!(
        error,
        TimelineStoreError::IndexNotInitialized { .. }
            | TimelineStoreError::ThreadNotIndexed { .. }
    )
}

/// Local mirror of the durable keyset cursor, which the runtime adapter must reconstruct from a
/// protocol `itemId`. The wire face of the timeline index is an opaque base64 cursor.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeCursor<'a> {
    version: u32,
    thread_id: &'a str,
    generation: &'a str,
    watermark: u64,
    ordinal: u64,
    slot_key: &'a str,
}

fn encode_cursor(
    thread_id: &str,
    generation: &str,
    watermark: u64,
    ordinal: u64,
    slot_key: &str,
) -> Result<String> {
    let cursor = RuntimeCursor {
        version: 1,
        thread_id,
        generation,
        watermark,
        ordinal,
        slot_key,
    };
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(serde_json::to_vec(&cursor)?))
}

/// Reassembles one display item from a bounded preview plus, when oversized, its chunked content.
async fn read_entry_item(
    reader: &TimelineReader,
    thread_id: &str,
    entry: &TimelineEntry,
) -> Result<ThreadItem> {
    if let Some(item) = entry.preview.decode_item() {
        return Ok(item);
    }
    let Some(reference) = entry.content.as_ref() else {
        bail!(
            "Thread {thread_id} timeline item {} is truncated without a content reference",
            entry.item_id
        );
    };
    let mut bytes = Vec::with_capacity(usize::try_from(reference.total_bytes).unwrap_or(0));
    let mut offset = 0u64;
    loop {
        let chunk = reader
            .read_content(thread_id, &reference.ref_id, offset, CONTENT_READ_BYTES)
            .await
            .map_err(|source| {
                anyhow::Error::new(TimelineUnavailable::Unavailable {
                    thread_id: thread_id.to_owned(),
                    source,
                })
            })?;
        if chunk.bytes.is_empty() {
            break;
        }
        bytes.extend_from_slice(&chunk.bytes);
        match chunk.next_offset {
            Some(next) if next > offset => offset = next,
            _ => break,
        }
    }
    anyhow::ensure!(
        bytes.len() as u64 == reference.total_bytes,
        "Thread {thread_id} timeline content for item {} is incomplete",
        entry.item_id
    );
    anyhow::ensure!(
        sha256_hex(&bytes) == reference.digest,
        "Thread {thread_id} timeline content digest does not match item {}",
        entry.item_id
    );
    serde_json::from_slice(&bytes)
        .map_err(|error| anyhow::anyhow!("decode Thread {thread_id} timeline item: {error}"))
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
        hex.push(char::from_digit(u32::from(byte & 0x0f), 16).unwrap_or('0'));
    }
    hex
}

impl StudioRuntime {
    /// Returns an item page, including related Turn metadata, without executing the Thread.
    ///
    /// # Errors
    /// Fails on unknown Thread/item identity or canonical storage/projection failure.
    pub async fn list_timeline_items(
        &self,
        thread_id: &str,
        query: TimelineQuery,
        limit: usize,
    ) -> Result<TimelinePage> {
        // Directory facts only: selecting a Thread never activates an owner (design/18 §18.3).
        let record = self.read_owned_thread(thread_id).await?;
        let parent_id = record.parent_thread_id.clone();
        let owner = self.threads.thread(thread_id);
        if let Some(owner) = &owner {
            if let Some(page) = self
                .resident_timeline_page(thread_id, &query, limit, owner.snapshot().commit_sequence)
                .await
            {
                return page;
            }
        }
        let Some(path) = self.session_database_path(thread_id)? else {
            return Err(anyhow::Error::new(TimelineUnavailable::NotDurable {
                thread_id: thread_id.to_owned(),
            }));
        };
        self.durable_timeline_page(
            thread_id,
            parent_id.as_deref(),
            &path,
            &query,
            limit,
            owner.is_some(),
        )
        .await
    }

    /// Per-Thread session database path, or `None` for an in-memory runtime without one.
    fn session_database_path(&self, thread_id: &str) -> Result<Option<PathBuf>> {
        let Some(dir) = self.store.sessions().sessions_dir() else {
            return Ok(None);
        };
        Ok(Some(crate::studio::paths::session_database_path(
            dir, thread_id,
        )?))
    }

    /// Serves the page from a resident owner's in-memory overlay when it is complete at the
    /// owner's watermark.
    ///
    /// The overlay shares the durable slot/watermark identity, so choosing between the resident and
    /// durable source never duplicates nor skips an item (design/17 §17.2). A stale overlay falls
    /// through to the durable index rather than serving an older watermark.
    async fn resident_timeline_page(
        &self,
        thread_id: &str,
        query: &TimelineQuery,
        limit: usize,
        owner_watermark: u64,
    ) -> Option<Result<TimelinePage>> {
        let indexes = self.residency.timelines.lock().await;
        let index = indexes.get(thread_id)?;
        if index.watermark() != owner_watermark {
            return None;
        }
        Some(index.page(thread_id, query, limit))
    }

    async fn durable_timeline_page(
        &self,
        thread_id: &str,
        parent_id: Option<&str>,
        path: &Path,
        query: &TimelineQuery,
        limit: usize,
        owner_present: bool,
    ) -> Result<TimelinePage> {
        let access = self.residency.timeline.clone();
        let reader = match access.reader(thread_id, path).await {
            Ok(reader) => reader,
            Err(error) => {
                return Err(self
                    .unavailable_or_prepare(thread_id, parent_id, path, error, owner_present)
                    .await);
            }
        };
        if let Err(error) = reader.read_head(thread_id).await {
            return Err(self
                .unavailable_or_prepare(thread_id, parent_id, path, error, owner_present)
                .await);
        }
        let page_query = self
            .timeline_page_query(&reader, thread_id, query, limit)
            .await?;
        let budget = TimelineBudget::new(limit.clamp(1, 100), DEFAULT_PAGE_BYTES)
            .map_err(|source| self.unavailable(thread_id, source))?;
        let window = reader
            .page(thread_id, &page_query, &budget)
            .await
            .map_err(|source| self.unavailable(thread_id, source))?;
        self.window_to_page(&reader, thread_id, window).await
    }

    fn unavailable(&self, thread_id: &str, source: TimelineStoreError) -> anyhow::Error {
        anyhow::Error::new(TimelineUnavailable::Unavailable {
            thread_id: thread_id.to_owned(),
            source,
        })
    }

    /// Turns a repairable index fault into a typed preparing/failure result, starting the worker.
    async fn unavailable_or_prepare(
        &self,
        thread_id: &str,
        parent_id: Option<&str>,
        path: &Path,
        error: TimelineStoreError,
        owner_present: bool,
    ) -> anyhow::Error {
        if !is_preparable(&error) {
            return self.unavailable(thread_id, error);
        }
        // A resident Thread's index is owned and advanced by its observation worker; starting a
        // second, whole-journal index worker would race that writer, so the query only reports the
        // typed preparing state and lets the owner's worker finish.
        if owner_present {
            return anyhow::Error::new(TimelineUnavailable::Preparing {
                thread_id: thread_id.to_owned(),
            });
        }
        let state = self
            .residency
            .timeline
            .kick(path.to_path_buf(), thread_id, parent_id.map(str::to_owned))
            .await;
        match state {
            IndexState::Failed(source) => anyhow::Error::new(TimelineUnavailable::Failed {
                thread_id: thread_id.to_owned(),
                source,
            }),
            _ => anyhow::Error::new(TimelineUnavailable::Preparing {
                thread_id: thread_id.to_owned(),
            }),
        }
    }

    async fn timeline_page_query(
        &self,
        reader: &TimelineReader,
        thread_id: &str,
        query: &TimelineQuery,
        limit: usize,
    ) -> Result<TimelinePageQuery> {
        Ok(match query {
            TimelineQuery::Latest => TimelinePageQuery::Latest,
            TimelineQuery::Around { item_id } => TimelinePageQuery::Around {
                item_id: item_id.clone(),
            },
            TimelineQuery::Before { item_id } => TimelinePageQuery::Before {
                cursor: self
                    .locate_item_cursor(reader, thread_id, item_id, limit)
                    .await?,
            },
            TimelineQuery::After { item_id } => TimelinePageQuery::After {
                cursor: self
                    .locate_item_cursor(reader, thread_id, item_id, limit)
                    .await?,
            },
        })
    }

    /// Resolves a protocol `itemId` cursor into the durable index's opaque keyset cursor.
    async fn locate_item_cursor(
        &self,
        reader: &TimelineReader,
        thread_id: &str,
        item_id: &str,
        limit: usize,
    ) -> Result<String> {
        let budget = TimelineBudget::new(limit.clamp(1, 100), DEFAULT_PAGE_BYTES)
            .map_err(|source| self.unavailable(thread_id, source))?;
        let window = reader
            .page(
                thread_id,
                &TimelinePageQuery::Around {
                    item_id: item_id.to_owned(),
                },
                &budget,
            )
            .await
            .map_err(|source| self.unavailable(thread_id, source))?;
        let entry = window
            .entries
            .iter()
            .find(|entry| entry.item_id == item_id)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Thread {thread_id} timeline item {item_id} is not part of the located page"
                )
            })?;
        encode_cursor(
            thread_id,
            &window.generation,
            window.read_watermark,
            entry.ordinal,
            &entry.slot_key,
        )
    }

    async fn window_to_page(
        &self,
        reader: &TimelineReader,
        thread_id: &str,
        window: TimelineWindow,
    ) -> Result<TimelinePage> {
        let mut items = Vec::with_capacity(window.entries.len());
        for entry in &window.entries {
            items.push(read_entry_item(reader, thread_id, entry).await?);
        }
        let older_cursor = window
            .older_cursor
            .as_ref()
            .and_then(|_| window.entries.first().map(|entry| entry.item_id.clone()));
        let newer_cursor = window
            .newer_cursor
            .as_ref()
            .and_then(|_| window.entries.last().map(|entry| entry.item_id.clone()));
        let turns = window
            .turns
            .into_iter()
            .map(|meta| TimelineTurn {
                turn: meta.turn,
                last_item_id: meta.last_item_id,
                context_disposition: meta.context_disposition,
            })
            .collect();
        Ok(TimelinePage {
            thread_id: thread_id.to_owned(),
            watermark: window.read_watermark,
            items,
            older_cursor,
            newer_cursor,
            first_item_id: window.entries.first().map(|entry| entry.item_id.clone()),
            last_item_id: window.entries.last().map(|entry| entry.item_id.clone()),
            turns,
        })
    }

    pub(super) async fn index_timeline(
        &self,
        thread_id: &str,
        state: &pl_core::thread::ThreadSnapshot,
        items: &[ThreadItem],
    ) {
        let ends: BTreeMap<_, _> = items
            .iter()
            .filter(|item| item.kind() != pl_protocol::ThreadItemKind::ContextCompaction)
            .map(|item| (item.turn_id.as_str(), item.id.as_str()))
            .collect();
        let turns = items.iter().filter_map(|item| {
            let pl_protocol::ThreadItemState::Turn(turn) = item.state() else {
                return None;
            };
            Some(pl_protocol::Turn {
                id: item.turn_id.clone(),
                thread_id: thread_id.into(),
                input_id: turn.input_id().map(str::to_owned),
                revision: item.revision,
                state: turn.state().clone(),
                updated_at: item.updated_at,
            })
        });
        let rolled_back = super::history::rolled_back_turns(state);
        let turns = turns
            .filter_map(|turn| {
                let last = ends.get(turn.id.as_str())?;
                Some(TimelineTurn {
                    context_disposition: if rolled_back.contains(&turn.id) {
                        pl_protocol::ThreadContextDisposition::RolledBack
                    } else {
                        pl_protocol::ThreadContextDisposition::Active
                    },
                    turn,
                    last_item_id: (*last).into(),
                })
            })
            .collect();
        self.residency
            .timelines
            .lock()
            .await
            .entry(thread_id.into())
            .or_default()
            .update(state.commit_sequence, items, turns);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_protocol::{ThreadContentLifecycle, ThreadItemState, ThreadTextChannel, ThreadTextItem};
    use pretty_assertions::assert_eq;

    fn items(count: u64) -> Vec<ThreadItem> {
        (0..count)
            .map(|ordinal| {
                ThreadItem::new(
                    format!("item-{ordinal}"),
                    "thread".into(),
                    "one-large-turn".into(),
                    ordinal,
                    1,
                    1,
                    1,
                    ThreadItemState::Text(ThreadTextItem::new(
                        ThreadTextChannel::Final,
                        format!("line {ordinal}\n代码 \\n"),
                        Vec::new(),
                        ThreadContentLifecycle::completed(1),
                    )),
                )
            })
            .collect()
    }

    #[test]
    fn large_turn_can_be_read_in_both_directions_without_gaps_or_repeated_pages() {
        let expected = items(1103);
        let mut index = TimelineIndex::default();
        index.update(1, &expected, Vec::new());
        let mut query = TimelineQuery::Latest;
        let mut collected = Vec::new();
        loop {
            let page = index.page("thread", &query, 100).unwrap();
            assert!(page.items.len() <= 100);
            let mut previous = page.items.clone();
            previous.extend(collected);
            collected = previous;
            let Some(item_id) = page.older_cursor else {
                break;
            };
            query = TimelineQuery::Before { item_id };
        }
        assert_eq!(collected, expected);
        let mut collected = Vec::new();
        let mut query = TimelineQuery::Around {
            item_id: "item-0".into(),
        };
        loop {
            let page = index.page("thread", &query, 100).unwrap();
            collected.extend(page.items);
            let Some(item_id) = page.newer_cursor else {
                break;
            };
            query = TimelineQuery::After { item_id };
        }
        assert_eq!(collected, expected);
        assert!(
            index
                .page(
                    "thread",
                    &TimelineQuery::Before {
                        item_id: "other-thread-item".into()
                    },
                    100
                )
                .is_err()
        );
        assert!(
            index
                .page(
                    "thread",
                    &TimelineQuery::Before {
                        item_id: "item-0".into()
                    },
                    100
                )
                .unwrap()
                .items
                .is_empty()
        );
        let around = index
            .page(
                "thread",
                &TimelineQuery::Around {
                    item_id: "item-500".into(),
                },
                100,
            )
            .unwrap();
        assert_eq!(around.items, expected[450..550]);
    }

    #[test]
    fn incremental_updates_keep_older_items_and_reject_an_older_watermark() {
        let mut index = TimelineIndex::default();
        let expected = items(4);
        index.update(2, &expected[..3], Vec::new());
        index.update(3, &expected[3..], Vec::new());
        index.update(1, &items(8), Vec::new());
        let page = index.page("thread", &TimelineQuery::Latest, 100).unwrap();
        assert_eq!(page.watermark, 3);
        assert_eq!(page.items, expected);
        assert_eq!(
            TimelineIndex::default()
                .page("thread", &TimelineQuery::Latest, 100)
                .unwrap()
                .items,
            Vec::new()
        );
    }

    /// A Thread whose durable index is missing reports a typed preparing state, and the explicit
    /// index worker repairs it without ever activating the Thread owner.
    #[tokio::test]
    async fn missing_index_reports_preparing_then_pages_after_the_worker_finishes() {
        let home = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let runtime = StudioRuntime::with_options(crate::StudioRuntimeOptions {
            studio_home: Some(home.path().to_owned()),
            host: crate::StudioHostKind::Test,
        })
        .await
        .unwrap();
        runtime.start_runtime().await.unwrap();
        let project = runtime.open_project(workspace.path()).await.unwrap();
        // Drain the accepted directory writes so the project row exists before seeding a Thread
        // directly through the store (which is only how this test bypasses owner activation).
        runtime
            .persistence_repository()
            .await
            .expect("a started test runtime owns a write-behind repository")
            .flush()
            .await
            .unwrap();
        // Thread creation owns the session database, but no derived index exists and no owner runs.
        let record = runtime
            .store
            .create_thread(
                &project.id,
                "cold index",
                pl_protocol::ThreadModeId::simple(),
            )
            .await
            .unwrap();
        assert!(runtime.threads.thread(&record.id).is_none());

        let error = runtime
            .list_timeline_items(&record.id, TimelineQuery::Latest, 20)
            .await
            .unwrap_err();
        assert!(
            matches!(
                error.downcast_ref::<TimelineUnavailable>(),
                Some(TimelineUnavailable::Preparing { .. })
            ),
            "a missing index must be a typed preparing state, never an empty page: {error:#}"
        );

        let page = loop {
            match runtime
                .list_timeline_items(&record.id, TimelineQuery::Latest, 20)
                .await
            {
                Ok(page) => break page,
                Err(error)
                    if matches!(
                        error.downcast_ref::<TimelineUnavailable>(),
                        Some(TimelineUnavailable::Preparing { .. })
                    ) =>
                {
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }
                Err(error) => panic!("index worker did not repair the index: {error:#}"),
            }
        };
        assert!(page.items.is_empty());
        assert_eq!(page.watermark, 0);
        // Serving the page from the durable index never activated the Thread.
        assert!(runtime.threads.thread(&record.id).is_none());
        assert!(
            runtime
                .threads
                .observed_threads()
                .iter()
                .all(|(id, _)| id != &record.id)
        );
        runtime.shutdown().await;
    }
}
