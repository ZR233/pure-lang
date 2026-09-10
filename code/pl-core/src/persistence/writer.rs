use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::FutureExt;
use tokio::sync::{Notify, watch};
use tokio::task::JoinHandle;
use tokio::time::Instant;

use super::{SessionPersistenceSnapshot, SessionStoreError, SqliteSessionOptions, sqlite};

/// Cloneable repository with an owned asynchronous writer. Call `shutdown` before dropping the last handle.
#[derive(Clone)]
pub struct SqliteSessionStore {
    pub(super) owner: Arc<Owner>,
}

impl std::fmt::Debug for SqliteSessionStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteSessionStore")
            .field("persistence", &self.persistence())
            .finish_non_exhaustive()
    }
}

pub(super) struct Owner {
    pub(super) shared: Arc<Shared>,
    task: Mutex<Option<JoinHandle<()>>>,
    database_lock: Mutex<Option<std::fs::File>>,
}

impl Drop for Owner {
    fn drop(&mut self) {
        if let Some(task) = self
            .task
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            task.abort();
        }
    }
}

pub(super) struct Shared {
    pub(super) db: sea_orm::DatabaseConnection,
    pub(super) state: Mutex<WriterState>,
    changed: watch::Sender<SessionPersistenceSnapshot>,
    wake: Notify,
}

pub(super) struct WriterState {
    resources: BTreeMap<(String, String), crate::storage::SessionEntry>,
    queue: VecDeque<Pending>,
    resource_admissions: BTreeMap<(String, String), u64>,
    admitted: u64,
    durable: u64,
    error: Option<Arc<SessionStoreError>>,
    flush: bool,
    stopping: bool,
    stopped: bool,
}

struct Pending {
    sequence: u64,
    accepted_at: Instant,
    retained_bytes: u64,
    operation: Arc<crate::storage::SessionEntry>,
}

impl SqliteSessionStore {
    /// Opens an independent database and starts its writer.
    ///
    /// # Errors
    /// Returns filesystem, database or schema errors without rebuilding an existing database.
    pub async fn open(options: SqliteSessionOptions) -> Result<Self, SessionStoreError> {
        let path = options.path.clone();
        let lock = tokio::task::spawn_blocking(move || {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let lock_path = path.with_extension("sqlite.lock");
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(lock_path)?;
            fs4::FileExt::try_lock(&file).map_err(|error| {
                SessionStoreError::Invalid(format!(
                    "session database already owned or cannot be locked: {error}"
                ))
            })?;
            Ok::<_, SessionStoreError>(file)
        })
        .await
        .map_err(|_| SessionStoreError::Panicked)??;
        Self::start(sqlite::open(Some(options)).await?, Some(lock)).await
    }

    /// Opens an ephemeral SQLite database with the same commit semantics.
    ///
    /// # Errors
    /// Returns a database initialization error.
    pub async fn open_memory() -> Result<Self, SessionStoreError> {
        Self::start(sqlite::open(None).await?, None).await
    }

    async fn start(
        db: sea_orm::DatabaseConnection,
        database_lock: Option<std::fs::File>,
    ) -> Result<Self, SessionStoreError> {
        use sea_orm::ConnectionTrait;
        let initial_state = async {
            let mut resources = BTreeMap::new();
            for row in db
                .query_all_raw(sqlite::statement(
                    "SELECT * FROM session_entries WHERE substr(id,1,12)='pl.resource.'",
                    vec![],
                ))
                .await?
            {
                let entry = sqlite::decode_row(row)?;
                resources.insert((entry.session_id.clone(), entry.id.clone()), entry);
            }
            Ok::<_, SessionStoreError>(resources)
        }
        .await;
        let resources = match initial_state {
            Ok(state) => state,
            Err(initialization) => {
                return match db.close().await {
                    Ok(()) => Err(initialization),
                    Err(cleanup) => Err(SessionStoreError::InitializationCleanup {
                        initialization: Box::new(initialization),
                        cleanup: Box::new(cleanup),
                    }),
                };
            }
        };
        let (changed, _) = watch::channel(SessionPersistenceSnapshot {
            pending_commits: 0,
            admitted: 0,
            durable: 0,
            error: None,
            stopped: false,
        });
        let shared = Arc::new(Shared {
            db,
            changed,
            wake: Notify::new(),
            state: Mutex::new(WriterState {
                resources,
                queue: VecDeque::new(),
                resource_admissions: BTreeMap::new(),
                admitted: 0,
                durable: 0,
                error: None,
                flush: false,
                stopping: false,
                stopped: false,
            }),
        });
        let task = tokio::spawn(supervise(shared.clone()));
        Ok(Self {
            owner: Arc::new(Owner {
                shared,
                task: Mutex::new(Some(task)),
                database_lock: Mutex::new(database_lock),
            }),
        })
    }

    /// Returns a coherent writer snapshot without touching SQLite.
    pub fn persistence(&self) -> SessionPersistenceSnapshot {
        self.owner.shared.changed.borrow().clone()
    }

    /// Returns retained encoded bytes for pressure admission; accepted work is never discarded.
    pub fn pending_bytes(&self, thread_id: &str) -> (u64, u64) {
        let state = self
            .owner
            .shared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state
            .queue
            .iter()
            .fold((0_u64, 0_u64), |(thread, store), pending| {
                let owner = pending.operation.session_id.as_str();
                (
                    if owner == thread_id {
                        thread.saturating_add(pending.retained_bytes)
                    } else {
                        thread
                    },
                    store.saturating_add(pending.retained_bytes),
                )
            })
    }

    /// Subscribes to writer progress and failures.
    pub fn subscribe_persistence(&self) -> watch::Receiver<SessionPersistenceSnapshot> {
        self.owner.shared.changed.subscribe()
    }

    /// Registers immutable session resource metadata through the core writer, independently of products.
    ///
    /// # Errors
    /// Rejects invalid resource identity or excessive payload size; storage failures are reported asynchronously.
    pub fn register_resource(
        &self,
        session_id: &str,
        id: &str,
        payload: crate::context::OpaquePayload,
    ) -> Result<(), super::ResourceAdmissionError> {
        if payload.content().len() > super::DEFAULT_RESOURCE_MAX_BYTES {
            return Err(super::ResourceAdmissionError::TooLarge {
                limit: super::DEFAULT_RESOURCE_MAX_BYTES,
            });
        }
        self.register_immutable_payload(session_id, id, payload)
    }

    /// Admits an already committed framework record, governed by queue pressure rather than metadata size.
    pub(super) fn register_immutable_payload(
        &self,
        session_id: &str,
        id: &str,
        payload: crate::context::OpaquePayload,
    ) -> Result<(), super::ResourceAdmissionError> {
        use super::ResourceAdmissionError;
        use crate::storage::SessionEntry;
        if session_id.is_empty()
            || id.is_empty()
            || payload.format().is_empty()
            || payload.version() == 0
        {
            return Err(ResourceAdmissionError::InvalidIdentity(id.into()));
        }
        let type_id = payload.format().to_owned();
        let schema_version = payload.version();
        let payload = payload.content().to_owned();
        let now = crate::time::unix_seconds();
        let mut state = self
            .owner
            .shared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let key = (session_id.to_owned(), format!("pl.resource.{id}"));
        if let Some(previous) = state.resources.get(&key) {
            if previous.type_id == type_id
                && previous.schema_version == schema_version
                && previous.payload == payload
            {
                return Ok(());
            }
            return Err(ResourceAdmissionError::Conflict {
                id: id.into(),
                expected: None,
                actual: Some(previous.revision),
            });
        }
        if state.stopping || state.stopped {
            return Err(ResourceAdmissionError::StoreClosed);
        }
        let resource_ordinal = state
            .resources
            .values()
            .filter(|entry| entry.session_id == session_id)
            .map(|entry| entry.ordinal)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(ResourceAdmissionError::RevisionExhausted)?;
        state.admitted = state
            .admitted
            .checked_add(1)
            .ok_or(ResourceAdmissionError::RevisionExhausted)?;
        let sequence = state.admitted;
        let entry = SessionEntry {
            session_id: session_id.into(),
            id: format!("pl.resource.{id}"),
            type_id,
            schema_version,
            ordinal: resource_ordinal,
            revision: 1,
            turn_id: None,
            created_at: now,
            updated_at: now,
            payload,
        };
        state.resource_admissions.insert(key.clone(), sequence);
        state.resources.insert(key, entry.clone());
        state.queue.push_back(Pending {
            sequence,
            accepted_at: Instant::now(),
            retained_bytes: entry.payload.len() as u64,
            operation: Arc::new(entry),
        });
        publish(&self.owner.shared, &state);
        drop(state);
        self.owner.shared.wake.notify_one();
        Ok(())
    }

    /// Reads registered immutable metadata from its memory owner, including unflushed records.
    pub fn resources(&self, session_id: &str, type_id: &str) -> Vec<crate::storage::SessionEntry> {
        self.owner
            .shared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .resources
            .values()
            .filter(|entry| entry.session_id == session_id && entry.type_id == type_id)
            .cloned()
            .collect()
    }

    fn request_flush(&self) {
        self.owner
            .shared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .flush = true;
        self.owner.shared.wake.notify_one();
    }

    /// Waits for one immutable record's admission watermark, independent of later queued records.
    ///
    /// # Errors
    /// Rejects unknown records and reports writer failure without dropping pending data.
    pub async fn flush_resource(
        &self,
        session_id: &str,
        record_id: &str,
    ) -> Result<(), Arc<SessionStoreError>> {
        let mut progress = self.subscribe_persistence();
        let target = {
            let state = self
                .owner
                .shared
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let key = (session_id.to_owned(), record_id.to_owned());
            if !state.resources.contains_key(&key) {
                return Err(Arc::new(SessionStoreError::Invalid(
                    "unknown immutable resource record".into(),
                )));
            }
            state.resource_admissions.get(&key).copied().unwrap_or(0)
        };
        self.request_flush();
        loop {
            let snapshot = progress.borrow().clone();
            if snapshot.durable >= target {
                return Ok(());
            }
            check_progress(&snapshot)?;
            progress
                .changed()
                .await
                .map_err(|_| Arc::new(SessionStoreError::Stopped))?;
        }
    }

    /// Flushes the admission watermark captured at invocation, not subsequent writes.
    ///
    /// # Errors
    /// Returns a writer error, retaining the pending checkpoints.
    pub async fn flush(&self) -> Result<(), Arc<SessionStoreError>> {
        let mut progress = self.subscribe_persistence();
        let target = progress.borrow().admitted;
        self.request_flush();
        loop {
            let snapshot = progress.borrow().clone();
            if snapshot.durable >= target {
                return Ok(());
            }
            check_progress(&snapshot)?;
            progress
                .changed()
                .await
                .map_err(|_| Arc::new(SessionStoreError::Stopped))?;
        }
    }

    /// Requests another attempt after the caller has repaired the cause of a blocked write.
    pub fn retry(&self) {
        let mut state = self
            .owner
            .shared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let restart =
            state.stopped && matches!(state.error.as_deref(), Some(SessionStoreError::Panicked));
        if state.stopped && !restart {
            return;
        }
        state.error = None;
        state.flush = true;
        if restart {
            state.stopped = false;
        }
        publish(&self.owner.shared, &state);
        drop(state);
        if restart {
            *self
                .owner
                .task
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) =
                Some(tokio::spawn(supervise(self.owner.shared.clone())));
        }
        self.owner.shared.wake.notify_one();
    }

    /// Drains and joins the writer, closes its database pool, then releases the file lock.
    /// A failure retains the owner and file lock for a later shutdown attempt.
    ///
    /// # Errors
    /// Returns an unconfirmed write or worker failure.
    pub async fn shutdown(&self) -> Result<(), Arc<SessionStoreError>> {
        self.flush().await?;
        let mut progress = self.subscribe_persistence();
        {
            let mut state = self
                .owner
                .shared
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.stopping = true;
            state.flush = true;
        }
        self.owner.shared.wake.notify_one();
        loop {
            let snapshot = progress.borrow().clone();
            if let Some(error) = snapshot.error {
                return Err(error);
            }
            if snapshot.stopped {
                break;
            }
            progress
                .changed()
                .await
                .map_err(|_| Arc::new(SessionStoreError::Stopped))?;
        }
        let task = self
            .owner
            .task
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(task) = task {
            task.await
                .map_err(|_| Arc::new(SessionStoreError::Panicked))?;
        }
        let final_state = self.persistence();
        if let Some(error) = final_state.error {
            return Err(error);
        }
        if final_state.pending_commits != 0 || final_state.durable != final_state.admitted {
            return Err(Arc::new(SessionStoreError::Stopped));
        }
        // Closing a clone closes the shared pool, including idle connections retained by readers.
        // Keep the process lock until SQLite has released every connection and its WAL handles.
        self.owner
            .shared
            .db
            .clone()
            .close()
            .await
            .map_err(|source| Arc::new(SessionStoreError::from(source)))?;
        self.owner
            .database_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        Ok(())
    }
}

fn check_progress(snapshot: &SessionPersistenceSnapshot) -> Result<(), Arc<SessionStoreError>> {
    if let Some(error) = &snapshot.error {
        return Err(error.clone());
    }
    if snapshot.stopped {
        return Err(Arc::new(SessionStoreError::Stopped));
    }
    Ok(())
}

fn publish(shared: &Shared, state: &WriterState) {
    shared.changed.send_replace(SessionPersistenceSnapshot {
        pending_commits: state.queue.len(),
        admitted: state.admitted,
        durable: state.durable,
        error: state.error.clone(),
        stopped: state.stopped,
    });
}

async fn supervise(shared: Arc<Shared>) {
    if std::panic::AssertUnwindSafe(run(&shared))
        .catch_unwind()
        .await
        .is_err()
    {
        let mut state = shared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.error = Some(Arc::new(SessionStoreError::Panicked));
        state.stopped = true;
        publish(&shared, &state);
    }
}

async fn run(shared: &Shared) {
    loop {
        let notified = shared.wake.notified();
        let batch = {
            let mut state = shared
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if state.stopping && state.queue.is_empty() {
                state.stopped = true;
                publish(shared, &state);
                return;
            }
            if state.error.as_ref().is_some_and(|error| !error.retryable()) {
                None
            } else if let Some(first) = state.queue.front() {
                if state.flush
                    || state.queue.len() >= 64
                    || first.accepted_at.elapsed() >= Duration::from_secs(5)
                {
                    Some(
                        state
                            .queue
                            .iter()
                            .take(64)
                            .map(|entry| (entry.sequence, entry.operation.clone()))
                            .collect::<Vec<_>>(),
                    )
                } else {
                    None
                }
            } else {
                state.flush = false;
                None
            }
        };
        let Some(batch) = batch else {
            let deadline = shared
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .queue
                .front()
                .map(|entry| entry.accepted_at + Duration::from_secs(5));
            if let Some(deadline) = deadline.filter(|deadline| *deadline > Instant::now()) {
                tokio::select! { () = notified => {}, () = tokio::time::sleep_until(deadline) => {} }
            } else {
                notified.await;
            }
            continue;
        };
        let commits = batch
            .iter()
            .map(|(_, operation)| operation.clone())
            .collect::<Vec<_>>();
        match sqlite::apply(&shared.db, &commits).await {
            Ok(()) => {
                let mut state = shared
                    .state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                for (sequence, operation) in batch {
                    state.queue.pop_front();
                    state.durable = sequence;
                    state
                        .resource_admissions
                        .remove(&(operation.session_id.clone(), operation.id.clone()));
                }
                state.error = None;
                publish(shared, &state);
            }
            Err(error) => {
                let retryable = error.retryable();
                {
                    let mut state = shared
                        .state
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    state.error = Some(Arc::new(error));
                    publish(shared, &state);
                }
                if retryable {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    }
}
