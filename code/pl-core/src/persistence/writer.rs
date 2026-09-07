use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::FutureExt;
use tokio::sync::{Notify, watch};
use tokio::task::JoinHandle;
use tokio::time::Instant;

use super::{SessionPersistenceSnapshot, SessionStoreError, SqliteSessionOptions, sqlite};
use crate::ThreadCommit;

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
    resources: BTreeMap<(String, String), crate::session::entry::SessionEntry>,
    queue: VecDeque<Pending>,
    admitted: u64,
    durable: u64,
    pub(super) revisions: BTreeMap<String, u64>,
    error: Option<Arc<SessionStoreError>>,
    flush: bool,
    stopping: bool,
    stopped: bool,
}

struct Pending {
    sequence: u64,
    accepted_at: Instant,
    operation: PendingOperation,
}

#[derive(Clone)]
pub(super) enum PendingOperation {
    Thread(Arc<ThreadCommit>),
    Resource(Arc<crate::session::entry::SessionEntry>),
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
        let rows = db.query_all_raw(sqlite::statement("SELECT session_id,MAX(revision) AS revision FROM session_receipts GROUP BY session_id",vec![])).await?;
        let mut revisions = BTreeMap::new();
        for row in rows {
            let revision = row.try_get::<i64>("", "revision")?;
            revisions.insert(
                row.try_get::<String>("", "session_id")?,
                u64::try_from(revision)
                    .map_err(|_| SessionStoreError::Invalid("negative durable revision".into()))?,
            );
        }
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
                admitted: 0,
                durable: 0,
                revisions,
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

    /// Subscribes to writer progress and failures.
    pub fn subscribe_persistence(&self) -> watch::Receiver<SessionPersistenceSnapshot> {
        self.owner.shared.changed.subscribe()
    }

    /// Preserves an already committed memory checkpoint. This performs no database I/O.
    pub(super) fn record(&self, commit: ThreadCommit) {
        let shared = &self.owner.shared;
        let mut state = shared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.admitted = state.admitted.saturating_add(1);
        let sequence = state.admitted;
        state.flush |= commit.persistence == crate::PersistenceClass::Settlement
            || matches!(
                commit.facts.context,
                Some(crate::ThreadContextMutation::Replace { .. })
            );
        state.queue.push_back(Pending {
            sequence,
            accepted_at: Instant::now(),
            operation: PendingOperation::Thread(Arc::new(commit)),
        });
        if state.stopped {
            state.error = Some(Arc::new(SessionStoreError::Stopped));
        }
        publish(shared, &state);
        drop(state);
        shared.wake.notify_one();
    }

    /// Registers immutable session resource metadata through the core writer, independently of products.
    ///
    /// # Errors
    /// Rejects invalid resource identity or payload encoding; storage failures are reported asynchronously.
    pub fn register_resource<T: crate::session::entry::SessionEntryPayload>(
        &self,
        session_id: &str,
        id: &str,
        payload: &T,
    ) -> Result<(), crate::session::entry::SessionEntryError> {
        use crate::session::entry::{SessionEntry, SessionEntryError};
        if session_id.is_empty()
            || id.is_empty()
            || !T::TYPE_ID.contains('.')
            || T::TYPE_ID.starts_with("pl.")
            || T::SCHEMA_VERSION == 0
        {
            return Err(SessionEntryError::InvalidIdentity(id.into()));
        }
        let payload = serde_json::to_value(payload)?;
        if serde_json::to_vec(&payload)?.len() > crate::session::entry::DEFAULT_ENTRY_MAX_BYTES {
            return Err(SessionEntryError::TooLarge {
                limit: crate::session::entry::DEFAULT_ENTRY_MAX_BYTES,
            });
        }
        let now = crate::time::unix_seconds();
        let mut state = self
            .owner
            .shared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let key = (session_id.to_owned(), format!("pl.resource.{id}"));
        if let Some(previous) = state.resources.get(&key) {
            if previous.type_id == T::TYPE_ID
                && previous.schema_version == T::SCHEMA_VERSION
                && previous.payload == payload
            {
                return Ok(());
            }
            return Err(SessionEntryError::Conflict {
                id: id.into(),
                expected: None,
                actual: Some(previous.revision),
            });
        }
        if state.stopping || state.stopped {
            return Err(SessionEntryError::Unbound);
        }
        let resource_ordinal = state
            .resources
            .values()
            .filter(|entry| entry.session_id == session_id)
            .map(|entry| entry.ordinal)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(SessionEntryError::RevisionExhausted)?;
        state.admitted = state.admitted.saturating_add(1);
        let sequence = state.admitted;
        let entry = SessionEntry {
            session_id: session_id.into(),
            id: format!("pl.resource.{id}"),
            type_id: T::TYPE_ID.into(),
            schema_version: T::SCHEMA_VERSION,
            ordinal: resource_ordinal,
            revision: 1,
            turn_id: None,
            created_at: now,
            updated_at: now,
            payload,
        };
        state.resources.insert(key, entry.clone());
        state.queue.push_back(Pending {
            sequence,
            accepted_at: Instant::now(),
            operation: PendingOperation::Resource(Arc::new(entry)),
        });
        publish(&self.owner.shared, &state);
        drop(state);
        self.owner.shared.wake.notify_one();
        Ok(())
    }

    /// Reads registered immutable metadata from its memory owner, including unflushed records.
    pub fn resources(
        &self,
        session_id: &str,
        type_id: &str,
    ) -> Vec<crate::session::entry::SessionEntry> {
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

    /// Returns whether a specific owner revision has been confirmed by SQLite.
    pub fn is_durable(&self, session_id: &str, revision: u64) -> bool {
        self.owner
            .shared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .revisions
            .get(session_id)
            .is_some_and(|saved| *saved >= revision)
    }

    /// Waits only for the target session revision; cancellation leaves pending writes intact.
    ///
    /// # Errors
    /// Returns the writer failure or premature stop. The caller may retry after repairing storage.
    pub async fn await_durable(
        &self,
        session_id: &str,
        revision: u64,
    ) -> Result<(), Arc<SessionStoreError>> {
        let mut progress = self.subscribe_persistence();
        self.request_flush();
        loop {
            if self.is_durable(session_id, revision) {
                return Ok(());
            }
            check_progress(&progress.borrow())?;
            progress
                .changed()
                .await
                .map_err(|_| Arc::new(SessionStoreError::Stopped))?;
        }
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

    /// Drains then joins the writer. A failed drain does not destroy its retryable owner.
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
                    if let PendingOperation::Thread(commit) = operation {
                        state.revisions.insert(
                            commit.agent_id.to_string(),
                            commit.next_state.snapshot.revision,
                        );
                    }
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
