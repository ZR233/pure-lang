//! Studio-owned routing and lifecycle for per-Thread session stores.
//!
//! Each root and child Thread owns `studio/sessions/<thread-id>.sqlite`. This
//! router derives that path from a validated Thread id, opens stores lazily
//! instead of pre-opening a writer for every session, and keeps the actively
//! written stores bounded by an explicit lease/pin model. It never evicts a store
//! that a live handle still owns, and aggregates every open store into one global
//! backpressure view. core stays product-free: it still owns only generic
//! single-database reads and writes.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use pl_core::context::OpaquePayload;
use pl_core::persistence::{
    ResourceAdmissionError, SessionPersistenceSnapshot, SessionStoreError, SqliteSessionOptions,
    SqliteSessionStore,
};
use pl_core::storage::SessionEntry;
use pl_core::thread::ThreadSnapshot;
use pl_core::thread::cold::{ColdStore, ColdStoreError, StoragePressure};
use pl_core::thread::journal::{self, ThreadCommit};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::studio::paths::{session_database_path, validate_storage_id};

/// Default cap on simultaneously open session stores. Cold reads never occupy a
/// slot because they close immediately; a new open may close an idle, unpinned,
/// drained store first, and otherwise fails explicitly rather than evicting a live one.
const DEFAULT_MAX_OPEN_STORES: usize = 64;

/// Routing, capacity, or underlying store failure for one Thread's session store.
#[derive(Debug, thiserror::Error)]
pub enum SessionStoreRouteError {
    #[error("invalid Thread id for session storage: {0}")]
    InvalidThreadId(String),
    #[error("Thread {thread_id} has no session database at {path}; history was not created")]
    MissingSessionDatabase { thread_id: String, path: PathBuf },
    #[error("session store router is stopping or closed")]
    Stopped,
    #[error("session store router already holds {limit} open stores; no idle store to close")]
    AtCapacity { limit: usize },
    #[error("session storage path is unsafe ({reason}): {path}")]
    UnsafePath { path: PathBuf, reason: String },
    #[error(transparent)]
    Store(Arc<SessionStoreError>),
    #[error(transparent)]
    Admission(#[from] ResourceAdmissionError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl From<SessionStoreError> for SessionStoreRouteError {
    fn from(error: SessionStoreError) -> Self {
        Self::Store(Arc::new(error))
    }
}

impl From<Arc<SessionStoreError>> for SessionStoreRouteError {
    fn from(error: Arc<SessionStoreError>) -> Self {
        Self::Store(error)
    }
}

/// Where per-Thread session databases live for this router.
#[derive(Debug, Clone)]
enum StoreHome {
    /// One file per Thread under this directory.
    Directory(PathBuf),
    /// One isolated in-memory database per Thread; used by tests and demo runs.
    Memory,
}

/// Cloneable per-Thread session store router with global write-behind backpressure.
#[derive(Clone)]
pub struct SessionStores {
    inner: Arc<SessionStoresInner>,
}

struct SessionStoresInner {
    home: StoreHome,
    max_open: usize,
    open: Mutex<OpenStores>,
    /// Serializes database opens and closes so one Thread never races a second open.
    open_gate: tokio::sync::Mutex<()>,
    changed: watch::Sender<SessionPersistenceSnapshot>,
    stopped: AtomicBool,
}

#[derive(Default)]
struct OpenStores {
    /// Actively written stores, keyed by Thread id.
    stores: BTreeMap<String, OpenStore>,
    /// Every Thread id ever opened, so in-memory routers can answer `session_ids`.
    known: BTreeSet<String>,
}

struct OpenStore {
    store: SqliteSessionStore,
    /// Aggregation forwarder; owns no facts and only holds a weak router handle.
    forwarder: JoinHandle<()>,
    /// Outstanding leases; a store with a live lease is never closed.
    pins: usize,
    /// Set while this store is being shut down; a concurrent acquire must not reuse it.
    closing: bool,
}

impl SessionStores {
    /// Routes file-backed session stores under `sessions_dir`, one per Thread.
    pub fn for_sessions_dir(sessions_dir: PathBuf) -> Self {
        Self::new(StoreHome::Directory(sessions_dir), DEFAULT_MAX_OPEN_STORES)
    }

    /// Routes isolated in-memory session stores, one per Thread.
    pub fn memory() -> Self {
        Self::new(StoreHome::Memory, usize::MAX)
    }

    fn new(home: StoreHome, max_open: usize) -> Self {
        let (changed, _) = watch::channel(SessionPersistenceSnapshot {
            pending_commits: 0,
            admitted: 0,
            durable: 0,
            error: None,
            stopped: false,
        });
        Self {
            inner: Arc::new(SessionStoresInner {
                home,
                max_open,
                open: Mutex::new(OpenStores::default()),
                open_gate: tokio::sync::Mutex::new(()),
                changed,
                stopped: AtomicBool::new(false),
            }),
        }
    }

    /// Directory holding per-Thread session databases, or `None` for in-memory routing.
    pub fn sessions_dir(&self) -> Option<&Path> {
        match &self.inner.home {
            StoreHome::Directory(dir) => Some(dir),
            StoreHome::Memory => None,
        }
    }

    /// Opens (creating for a new Thread) and pins the per-Thread store.
    ///
    /// The returned lease keeps the store alive and unpinned by idle closure for
    /// as long as a Thread owns its write-behind handle.
    ///
    /// # Errors
    /// Rejects an invalid Thread id, a stopped router, a full router with no idle
    /// store to close, or a store that cannot be opened.
    pub async fn open_thread(
        &self,
        thread_id: &str,
    ) -> Result<SessionStoreLease, SessionStoreRouteError> {
        if self.inner.stopped.load(Ordering::Acquire) {
            return Err(SessionStoreRouteError::Stopped);
        }
        let path = self.validated_database_path(thread_id)?;
        if let Some(path) = &path {
            reject_link_chain(
                "Thread session database",
                self.sessions_dir().unwrap_or(Path::new(".")),
                path,
            )
            .await?;
        }
        if let Some(lease) = self.acquire(thread_id) {
            return Ok(lease);
        }
        let _gate = self.inner.open_gate.lock().await;
        if self.inner.stopped.load(Ordering::Acquire) {
            return Err(SessionStoreRouteError::Stopped);
        }
        if let Some(lease) = self.acquire(thread_id) {
            return Ok(lease);
        }
        self.make_room(thread_id).await?;
        let store = match path {
            Some(path) => SqliteSessionStore::open(SqliteSessionOptions { path }).await?,
            None => SqliteSessionStore::open_memory().await?,
        };
        self.insert(thread_id.to_owned(), store);
        self.acquire(thread_id)
            .ok_or(SessionStoreRouteError::Stopped)
    }

    /// Opens and pins an existing per-Thread store; a missing history database is an error.
    ///
    /// Used by recovery, observation and existing-attachment reads, which must never
    /// create an empty journal for an existing Thread. Only a Thread creation command
    /// calls [`Self::open_thread`], which may create the database.
    ///
    /// # Errors
    /// Rejects an invalid Thread id, a stopped router, a missing database, or a full
    /// router with no idle store to close.
    pub async fn open_existing(
        &self,
        thread_id: &str,
    ) -> Result<SessionStoreLease, SessionStoreRouteError> {
        if self.inner.stopped.load(Ordering::Acquire) {
            return Err(SessionStoreRouteError::Stopped);
        }
        if let Some(path) = self.validated_database_path(thread_id)? {
            reject_link_chain(
                "Thread session database",
                self.sessions_dir().unwrap_or(Path::new(".")),
                &path,
            )
            .await?;
        }
        if let Some(lease) = self.acquire(thread_id) {
            return Ok(lease);
        }
        let _gate = self.inner.open_gate.lock().await;
        if self.inner.stopped.load(Ordering::Acquire) {
            return Err(SessionStoreRouteError::Stopped);
        }
        if let Some(lease) = self.acquire(thread_id) {
            return Ok(lease);
        }
        self.make_room(thread_id).await?;
        let store = self.open_existing_uncached(thread_id).await?;
        self.insert(thread_id.to_owned(), store);
        self.acquire(thread_id)
            .ok_or(SessionStoreRouteError::Stopped)
    }

    /// Registers immutable metadata on a Thread's own session store.
    ///
    /// # Errors
    /// Rejects an invalid Thread id, a stopped router or an admission conflict.
    pub async fn register_resource(
        &self,
        thread_id: &str,
        id: &str,
        payload: OpaquePayload,
    ) -> Result<(), SessionStoreRouteError> {
        let lease = self.open_thread(thread_id).await?;
        lease.store.register_resource(thread_id, id, payload)?;
        Ok(())
    }

    /// Reads a Thread journal through an active writer or a transient reader.
    ///
    /// A missing history database is an explicit error and is never created.
    ///
    /// # Errors
    /// Rejects an invalid Thread id, a missing database, or corrupt history.
    pub async fn read_thread_journal(
        &self,
        thread_id: &str,
    ) -> Result<Vec<Arc<ThreadCommit>>, SessionStoreRouteError> {
        if let Some(lease) = self.acquire(thread_id) {
            return lease
                .store
                .read_thread_journal(thread_id)
                .await
                .map_err(Into::into);
        }
        let _gate = self.inner.open_gate.lock().await;
        if let Some(lease) = self.acquire(thread_id) {
            return lease
                .store
                .read_thread_journal(thread_id)
                .await
                .map_err(Into::into);
        }
        let store = self.open_existing_uncached(thread_id).await?;
        let outcome = store.read_thread_journal(thread_id).await;
        let closed = store.shutdown().await;
        let history = outcome.map_err(SessionStoreRouteError::from)?;
        closed.map_err(SessionStoreRouteError::from)?;
        Ok(history)
    }

    /// Replays a Thread journal into a snapshot without activating a model or tool.
    ///
    /// # Errors
    /// Rejects an invalid Thread id, a missing database or invalid history.
    pub async fn replay_thread(
        &self,
        thread_id: &str,
    ) -> Result<ThreadSnapshot, SessionStoreRouteError> {
        let history = self.read_thread_journal(thread_id).await?;
        journal::replay(&history).map_err(|error| {
            SessionStoreRouteError::from(SessionStoreError::Invalid(error.to_string()))
        })
    }

    /// Reads registered immutable metadata for one Thread, opening it only if needed.
    ///
    /// # Errors
    /// Rejects an invalid Thread id or a missing database.
    pub async fn resources(
        &self,
        thread_id: &str,
        type_id: &str,
    ) -> Result<Vec<SessionEntry>, SessionStoreRouteError> {
        if let Some(lease) = self.acquire(thread_id) {
            return Ok(lease.store.resources(thread_id, type_id));
        }
        let _gate = self.inner.open_gate.lock().await;
        if let Some(lease) = self.acquire(thread_id) {
            return Ok(lease.store.resources(thread_id, type_id));
        }
        let store = self.open_existing_uncached(thread_id).await?;
        let records = store.resources(thread_id, type_id);
        store
            .shutdown()
            .await
            .map_err(SessionStoreRouteError::from)?;
        Ok(records)
    }

    /// Replays a Thread journal and appends the deterministic recovery settlement when needed.
    ///
    /// Reuses an active writer, or opens a transient store that is drained and
    /// closed before returning, so a startup audit never pre-opens every session.
    ///
    /// # Errors
    /// Rejects a missing database, corrupt history or a settlement that cannot be stored.
    pub async fn recover_thread_journal(
        &self,
        thread_id: &str,
    ) -> Result<Vec<Arc<ThreadCommit>>, SessionStoreRouteError> {
        if let Some(lease) = self.acquire(thread_id) {
            return recover_on(&lease.store, thread_id).await;
        }
        let _gate = self.inner.open_gate.lock().await;
        if let Some(lease) = self.acquire(thread_id) {
            return recover_on(&lease.store, thread_id).await;
        }
        let store = self.open_existing_uncached(thread_id).await?;
        let outcome = recover_on(&store, thread_id).await;
        let closed = store.shutdown().await;
        let history = outcome?;
        closed.map_err(SessionStoreRouteError::from)?;
        Ok(history)
    }

    /// Lists durable Thread identities without activating any actor.
    ///
    /// # Errors
    /// Returns filesystem errors while reading the sessions directory.
    pub async fn session_ids(&self) -> Result<Vec<String>, SessionStoreRouteError> {
        match &self.inner.home {
            StoreHome::Memory => Ok(self.lock_open().known.iter().cloned().collect()),
            StoreHome::Directory(dir) => {
                let mut entries = match tokio::fs::read_dir(dir).await {
                    Ok(entries) => entries,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        return Ok(Vec::new());
                    }
                    Err(error) => return Err(error.into()),
                };
                let mut ids = Vec::new();
                while let Some(entry) = entries.next_entry().await? {
                    let name = entry.file_name();
                    let Some(name) = name.to_str() else {
                        continue;
                    };
                    let Some(id) = name.strip_suffix(".sqlite") else {
                        continue;
                    };
                    if validate_storage_id("Thread", id).is_ok() {
                        ids.push(id.to_owned());
                    }
                }
                ids.sort();
                Ok(ids)
            }
        }
    }

    /// Aggregated writer snapshot over every open store, for global backpressure.
    ///
    /// `pending_commits`, `admitted` and `durable` are totals across open stores,
    /// not a single watermark; `error` is the first observed failure.
    pub fn persistence(&self) -> SessionPersistenceSnapshot {
        self.aggregate()
    }

    /// Subscribes to aggregated writer progress and failures.
    pub fn subscribe_persistence(&self) -> watch::Receiver<SessionPersistenceSnapshot> {
        self.inner.changed.subscribe()
    }

    /// Requests another durable attempt for every open Thread.
    pub fn retry(&self) {
        for entry in self.lock_open().stores.values() {
            entry.store.retry();
        }
    }

    /// Flushes every open Thread's admission watermark.
    ///
    /// # Errors
    /// Returns the first writer failure; pending facts stay owned in memory.
    pub async fn flush(&self) -> Result<(), Arc<SessionStoreError>> {
        // Hold a lease on every open store across the await so a concurrent close cannot
        // drain a store mid-flush.
        let mut leases = Vec::new();
        for id in self.store_ids() {
            if let Some(lease) = self.acquire(&id) {
                leases.push(lease);
            }
        }
        for lease in &leases {
            lease.store.flush().await?;
        }
        Ok(())
    }

    /// Drains writers, closes pools, then releases the database locks of every open Thread.
    ///
    /// Only stores whose writer drained are removed; a store that fails shutdown
    /// keeps its writer and file lock so a later call can retry it, and no new
    /// open can slip in after the stopped flag is published.
    ///
    /// # Errors
    /// Returns the first unconfirmed write; failed stores stay owned for retry.
    pub async fn shutdown(&self) -> Result<(), Arc<SessionStoreError>> {
        let _gate = self.inner.open_gate.lock().await;
        self.inner.stopped.store(true, Ordering::Release);
        // Refuse every new pin before shutting any store down, so no acquire can hand out a
        // store that is being closed.
        let ids: Vec<String> = {
            let mut open = self.lock_open();
            for entry in open.stores.values_mut() {
                entry.closing = true;
            }
            open.stores.keys().cloned().collect()
        };
        let mut first_error = None;
        for id in ids {
            let store = self
                .lock_open()
                .stores
                .get(&id)
                .map(|entry| entry.store.clone());
            let Some(store) = store else {
                continue;
            };
            match store.shutdown().await {
                Ok(()) => {
                    let removed = {
                        let mut open = self.lock_open();
                        open.stores.remove(&id)
                    };
                    if let Some(entry) = removed {
                        entry.forwarder.abort();
                        let _ = entry.forwarder.await;
                    }
                }
                Err(error) => {
                    if let Some(entry) = self.lock_open().stores.get_mut(&id) {
                        entry.closing = false;
                    }
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }
        self.publish_aggregate();
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    async fn make_room(&self, thread_id: &str) -> Result<(), SessionStoreRouteError> {
        loop {
            let idle = {
                let open = self.lock_open();
                let needs_room = !open.stores.contains_key(thread_id)
                    && open.stores.len() >= self.inner.max_open;
                if !needs_room {
                    break;
                }
                open.stores
                    .iter()
                    .find(|(_, entry)| {
                        entry.pins == 0
                            && !entry.closing
                            && entry.store.persistence().pending_commits == 0
                    })
                    .map(|(id, _)| id.clone())
            };
            match idle {
                Some(id) => self.close_idle(&id).await?,
                None => {
                    return Err(SessionStoreRouteError::AtCapacity {
                        limit: self.inner.max_open,
                    });
                }
            }
        }
        Ok(())
    }

    /// Drains and closes an unpinned, drained store, then drops its cache reference.
    /// A shutdown failure keeps the store owned so a later attempt can retry it.
    async fn close_idle(&self, idle: &str) -> Result<(), SessionStoreRouteError> {
        // Atomically reserve the store for closing only while it is unpinned and drained;
        // `closing` makes a concurrent acquire refuse it instead of reusing a shutdown store.
        let store = {
            let mut open = self.lock_open();
            let Some(entry) = open.stores.get_mut(idle) else {
                return Ok(());
            };
            if entry.closing || entry.pins > 0 || entry.store.persistence().pending_commits > 0 {
                return Ok(());
            }
            entry.closing = true;
            entry.store.clone()
        };
        match store.shutdown().await {
            Ok(()) => {
                let removed = self.lock_open().stores.remove(idle);
                if let Some(entry) = removed {
                    entry.forwarder.abort();
                    let _ = entry.forwarder.await;
                }
            }
            Err(error) => {
                // Keep ownership for retry and allow future pins again.
                if let Some(entry) = self.lock_open().stores.get_mut(idle) {
                    entry.closing = false;
                }
                self.publish_aggregate();
                return Err(SessionStoreRouteError::from(error));
            }
        }
        self.publish_aggregate();
        Ok(())
    }

    async fn open_existing_uncached(
        &self,
        thread_id: &str,
    ) -> Result<SqliteSessionStore, SessionStoreRouteError> {
        let path = self.validated_database_path(thread_id)?;
        let Some(path) = path else {
            // In-memory routing has no durable file: an unopened Thread has no history.
            return Err(SessionStoreRouteError::MissingSessionDatabase {
                thread_id: thread_id.to_owned(),
                path: PathBuf::new(),
            });
        };
        let root = self.sessions_dir().unwrap_or(Path::new("."));
        reject_link_chain("Thread session database", root, &path).await?;
        if !tokio::fs::try_exists(&path).await? {
            return Err(SessionStoreRouteError::MissingSessionDatabase {
                thread_id: thread_id.to_owned(),
                path,
            });
        }
        Ok(SqliteSessionStore::open_existing(SqliteSessionOptions { path }).await?)
    }

    fn validated_database_path(
        &self,
        thread_id: &str,
    ) -> Result<Option<PathBuf>, SessionStoreRouteError> {
        match &self.inner.home {
            StoreHome::Directory(dir) => session_database_path(dir, thread_id)
                .map(Some)
                .map_err(|error| SessionStoreRouteError::InvalidThreadId(error.to_string())),
            StoreHome::Memory => validate_storage_id("Thread", thread_id)
                .map(|()| None)
                .map_err(|error| SessionStoreRouteError::InvalidThreadId(error.to_string())),
        }
    }

    /// Atomically looks up a live store and pins it in one critical section.
    ///
    /// A closing or stopped store is never returned, so a concurrent close cannot hand out
    /// a shutdown lease; the returned lease keeps the store out of idle closure.
    fn acquire(&self, thread_id: &str) -> Option<SessionStoreLease> {
        if self.inner.stopped.load(Ordering::Acquire) {
            return None;
        }
        let mut open = self.lock_open();
        let entry = open.stores.get_mut(thread_id)?;
        if entry.closing {
            return None;
        }
        entry.pins += 1;
        let store = entry.store.clone();
        Some(SessionStoreLease {
            stores: self.clone(),
            thread_id: thread_id.to_owned(),
            store,
        })
    }

    fn store_ids(&self) -> Vec<String> {
        self.lock_open().stores.keys().cloned().collect()
    }

    fn release_pin(&self, thread_id: &str) {
        let mut open = self.lock_open();
        if let Some(entry) = open.stores.get_mut(thread_id) {
            entry.pins = entry.pins.saturating_sub(1);
        }
    }

    fn insert(&self, thread_id: String, store: SqliteSessionStore) {
        let mut receiver = store.subscribe_persistence();
        // A weak router handle keeps the forwarder from forming an ownership cycle;
        // it also ends the task once the reconciled router or store disappears.
        let weak = Arc::downgrade(&self.inner);
        let forwarder = tokio::spawn(async move {
            loop {
                if receiver.changed().await.is_err() {
                    break;
                }
                let Some(inner) = weak.upgrade() else {
                    break;
                };
                SessionStores { inner }.publish_aggregate();
            }
        });
        {
            let mut open = self.lock_open();
            open.known.insert(thread_id.clone());
            open.stores.insert(
                thread_id,
                OpenStore {
                    store,
                    forwarder,
                    pins: 0,
                    closing: false,
                },
            );
        }
        self.publish_aggregate();
    }

    fn publish_aggregate(&self) {
        self.inner.changed.send_replace(self.aggregate());
    }

    fn aggregate(&self) -> SessionPersistenceSnapshot {
        let open = self.lock_open();
        let mut pending_commits = 0usize;
        let mut admitted = 0u64;
        let mut durable = 0u64;
        let mut error = None;
        for entry in open.stores.values() {
            let snapshot = entry.store.persistence();
            pending_commits = pending_commits.saturating_add(snapshot.pending_commits);
            admitted = admitted.saturating_add(snapshot.admitted);
            durable = durable.saturating_add(snapshot.durable);
            if error.is_none() {
                error = snapshot.error;
            }
        }
        SessionPersistenceSnapshot {
            pending_commits,
            admitted,
            durable,
            error,
            stopped: self.inner.stopped.load(Ordering::Acquire),
        }
    }

    fn lock_open(&self) -> std::sync::MutexGuard<'_, OpenStores> {
        self.inner
            .open
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

/// Pinned lease over one Thread's session store.
///
/// A live lease keeps the store eligible for the aggregate view but exempt from
/// idle closure; holding the store also keeps its writer and file lock alive for
/// the owning Thread.
pub struct SessionStoreLease {
    stores: SessionStores,
    thread_id: String,
    store: SqliteSessionStore,
}

impl Drop for SessionStoreLease {
    fn drop(&mut self) {
        self.stores.release_pin(&self.thread_id);
    }
}

impl std::fmt::Debug for SessionStoreLease {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionStoreLease")
            .field("thread_id", &self.thread_id)
            .field("persistence", &self.store.persistence())
            .finish_non_exhaustive()
    }
}

impl ColdStore for SessionStoreLease {
    fn pressure(&self, thread_id: &str) -> StoragePressure {
        // Route through the router so global byte pressure spans every open store.
        self.stores.pressure(thread_id)
    }

    fn admit(
        &self,
        thread_id: &str,
        sequence: u64,
        payload: OpaquePayload,
    ) -> Result<(), ColdStoreError> {
        self.store.admit(thread_id, sequence, payload)
    }

    fn flush(
        &self,
        thread_id: &str,
        sequence: u64,
    ) -> impl std::future::Future<Output = Result<(), ColdStoreError>> + Send {
        let store = self.store.clone();
        let thread_id = thread_id.to_owned();
        async move { ColdStore::flush(&store, &thread_id, sequence).await }
    }
}

/// Reads a Thread journal and appends the deterministic recovery settlement when required.
async fn recover_on(
    store: &SqliteSessionStore,
    thread_id: &str,
) -> Result<Vec<Arc<ThreadCommit>>, SessionStoreRouteError> {
    let mut history = store.read_thread_journal(thread_id).await?;
    if let Some(commit) = journal::recovery_commit(&history)
        .map_err(|error| SessionStoreError::Invalid(error.to_string()))?
    {
        ColdStore::admit(
            store,
            thread_id,
            commit.sequence,
            commit
                .encode()
                .map_err(|error| SessionStoreError::Invalid(error.to_string()))?,
        )
        .map_err(|error| SessionStoreError::Invalid(error.to_string()))?;
        // Publication cannot overtake durable settlement.
        ColdStore::flush(store, thread_id, commit.sequence)
            .await
            .map_err(|error| SessionStoreError::Invalid(error.to_string()))?;
        history.push(Arc::new(commit));
    }
    Ok(history)
}

impl ColdStore for SessionStores {
    fn pressure(&self, thread_id: &str) -> StoragePressure {
        let open = self.lock_open();
        let mut thread_bytes = 0u64;
        let mut store_bytes = 0u64;
        let mut error = None;
        for (id, entry) in &open.stores {
            let (own, total) = entry.store.pending_bytes(id);
            store_bytes = store_bytes.saturating_add(total);
            if id == thread_id {
                thread_bytes = own;
            }
            if error.is_none() {
                error = entry.store.persistence().error.map(|source| {
                    Arc::new(ColdStoreError {
                        source: Box::new(source),
                    })
                });
            }
        }
        StoragePressure {
            thread_bytes,
            store_bytes,
            error,
        }
    }

    fn admit(
        &self,
        thread_id: &str,
        sequence: u64,
        payload: OpaquePayload,
    ) -> Result<(), ColdStoreError> {
        match self.acquire(thread_id) {
            Some(lease) => lease.store.admit(thread_id, sequence, payload),
            None => Err(ColdStoreError {
                source: Box::new(SessionStoreRouteError::MissingSessionDatabase {
                    thread_id: thread_id.to_owned(),
                    path: PathBuf::new(),
                }),
            }),
        }
    }

    fn flush(
        &self,
        thread_id: &str,
        sequence: u64,
    ) -> impl std::future::Future<Output = Result<(), ColdStoreError>> + Send {
        let lease = self.acquire(thread_id);
        let thread_id = thread_id.to_owned();
        async move {
            match lease {
                // A Thread with no open writer has nothing pending to flush.
                None => Ok(()),
                Some(lease) => ColdStore::flush(&lease.store, &thread_id, sequence).await,
            }
        }
    }
}

impl std::fmt::Debug for SessionStores {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionStores")
            .field("sessions_dir", &self.sessions_dir())
            .field("persistence", &self.persistence())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file_router(sessions_dir: &Path, max_open: usize) -> SessionStores {
        SessionStores::new(StoreHome::Directory(sessions_dir.to_path_buf()), max_open)
    }

    #[tokio::test]
    async fn root_and_child_threads_own_separate_databases() {
        let home = tempfile::tempdir().unwrap();
        let sessions_dir = home.path().join("sessions");
        let router = SessionStores::for_sessions_dir(sessions_dir.clone());

        let root = router.open_thread("thread-root").await.unwrap();
        router
            .register_resource(
                "thread-root",
                "attachment-1",
                OpaquePayload::new("studio.attachment", 1, "{}").unwrap(),
            )
            .await
            .unwrap();
        let child = router.open_thread("thread-child").await.unwrap();

        assert!(sessions_dir.join("thread-root.sqlite").exists());
        assert!(sessions_dir.join("thread-child.sqlite").exists());
        assert_eq!(
            router.session_ids().await.unwrap(),
            vec!["thread-child".to_string(), "thread-root".to_string()]
        );
        // Immutable metadata stays in the owning Thread's database only.
        assert_eq!(
            router
                .resources("thread-root", "studio.attachment")
                .await
                .unwrap()
                .len(),
            1
        );
        assert!(
            router
                .resources("thread-child", "studio.attachment")
                .await
                .unwrap()
                .is_empty()
        );

        drop(root);
        drop(child);
        router.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn cold_read_of_a_missing_database_errors_without_creating_it() {
        let home = tempfile::tempdir().unwrap();
        let sessions_dir = home.path().join("sessions");
        let router = SessionStores::for_sessions_dir(sessions_dir.clone());

        let error = router
            .read_thread_journal("thread-absent")
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            SessionStoreRouteError::MissingSessionDatabase { .. }
        ));
        assert!(!sessions_dir.join("thread-absent.sqlite").exists());
    }

    #[tokio::test]
    async fn a_pinned_store_is_never_closed_but_an_idle_store_is() {
        let home = tempfile::tempdir().unwrap();
        let sessions_dir = home.path().join("sessions");
        let router = file_router(&sessions_dir, 1);

        // Hold a lease on "a" so it is active; capacity admission must not close it.
        let active = router.open_thread("thread-a").await.unwrap();
        let error = router.open_thread("thread-b").await.unwrap_err();
        assert!(matches!(
            error,
            SessionStoreRouteError::AtCapacity { limit: 1 }
        ));
        assert!(sessions_dir.join("thread-a.sqlite").exists());

        // Releasing the lease makes "a" idle; the next open closes it safely first.
        drop(active);
        let b = router.open_thread("thread-b").await.unwrap();
        assert!(sessions_dir.join("thread-b.sqlite").exists());
        assert!(sessions_dir.join("thread-a.sqlite").exists());

        drop(b);
        router.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn global_pressure_sums_every_open_store() {
        let router = SessionStores::memory();
        let first = router.open_thread("first").await.unwrap();
        let second = router.open_thread("second").await.unwrap();
        ColdStore::admit(&first, "first", 1, OpaquePayload::text("first")).unwrap();
        ColdStore::admit(&second, "second", 1, OpaquePayload::text("second")).unwrap();

        // No await precedes these observations: the writer cannot run on this runtime.
        let pressure = ColdStore::pressure(&router, "first");
        assert_eq!(pressure.thread_bytes, 5);
        assert_eq!(pressure.store_bytes, 11);
        assert_eq!(router.persistence().pending_commits, 2);

        drop(first);
        drop(second);
        router.shutdown().await.unwrap();
        assert!(router.persistence().stopped);
    }

    #[tokio::test]
    async fn invalid_thread_ids_are_rejected_before_any_database_is_opened() {
        let router = SessionStores::memory();
        for id in ["", "..", "../escape", "a/b"] {
            assert!(matches!(
                router.open_thread(id).await.unwrap_err(),
                SessionStoreRouteError::InvalidThreadId(_)
            ));
        }
    }

    #[tokio::test]
    async fn pin_wins_then_close_wins_and_a_reopened_store_is_usable() {
        let home = tempfile::tempdir().unwrap();
        let sessions_dir = home.path().join("sessions");
        let router = file_router(&sessions_dir, 1);

        // Pin wins: a pinned store is never selected for idle close.
        let pinned = router.open_thread("thread-a").await.unwrap();
        assert!(matches!(
            router.open_thread("thread-b").await.unwrap_err(),
            SessionStoreRouteError::AtCapacity { limit: 1 }
        ));

        // Close wins: after the pin drops, opening another Thread closes the idle store.
        drop(pinned);
        let b = router.open_thread("thread-b").await.unwrap();
        // Release `b` so the idle close can evict it when `a` is reopened.
        drop(b);

        // Reopening the closed Thread yields a live, usable store, not a shutdown handle.
        let reopened = router.open_thread("thread-a").await.unwrap();
        ColdStore::admit(&reopened, "thread-a", 1, OpaquePayload::text("fact")).unwrap();
        ColdStore::flush(&reopened, "thread-a", 1).await.unwrap();
        assert_eq!(
            router.session_ids().await.unwrap(),
            vec!["thread-a".to_string(), "thread-b".to_string()]
        );

        drop(reopened);
        router.shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_barrier_orders_pin_and_close_without_sleeping() {
        use tokio::sync::Barrier;

        let home = tempfile::tempdir().unwrap();
        let sessions_dir = home.path().join("sessions");
        let router = file_router(&sessions_dir, 1);
        let barrier = Arc::new(Barrier::new(2));

        let holder_router = router.clone();
        let holder_barrier = barrier.clone();
        let holder = tokio::spawn(async move {
            let lease = holder_router.open_thread("thread-a").await.unwrap();
            holder_barrier.wait().await; // the pin is held
            holder_barrier.wait().await; // the observer saw AtCapacity
            drop(lease);
        });

        barrier.wait().await; // pin held
        assert!(matches!(
            router.open_thread("thread-b").await.unwrap_err(),
            SessionStoreRouteError::AtCapacity { limit: 1 }
        ));
        barrier.wait().await; // release the holder
        holder.await.unwrap();

        // Close wins after the pin drops; both databases remain on disk.
        let b = router.open_thread("thread-b").await.unwrap();
        assert!(sessions_dir.join("thread-a.sqlite").exists());
        assert!(sessions_dir.join("thread-b.sqlite").exists());
        drop(b);
        router.shutdown().await.unwrap();
    }
}

/// Rejects a symlink or reparse point on `path` or any existing ancestor up to `root`.
async fn reject_link_chain(
    label: &str,
    root: &Path,
    path: &Path,
) -> Result<(), SessionStoreRouteError> {
    let mut current = Some(path.to_path_buf());
    while let Some(candidate) = current {
        if let Ok(metadata) = tokio::fs::symlink_metadata(&candidate).await
            && pl_tool::workspace::path_safety::is_link_or_reparse(&metadata)
        {
            return Err(SessionStoreRouteError::UnsafePath {
                path: candidate.clone(),
                reason: format!("{label} must not be a symbolic link or reparse point"),
            });
        }
        if candidate == root {
            break;
        }
        current = candidate.parent().map(Path::to_path_buf);
    }
    Ok(())
}
