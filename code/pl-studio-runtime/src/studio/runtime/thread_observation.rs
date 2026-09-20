//! Owned, retryable product projections of the immutable Thread journal.
mod billing;
mod directory;
mod reports;

use super::{ModelPerformanceOwner, StudioRuntime};
use crate::studio::thread_projection::engine::{Facts, ProjectionDelta, ProjectionState};
use crate::studio::{ProductEventBus, StudioStore};
use anyhow::{Context, Result, bail};
use pl_core::thread::{
    ThreadHandle, ThreadLifecycle, ThreadSnapshot, TurnState, journal::ThreadCommit,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroUsize,
    sync::{Arc, Mutex},
};
use tokio::{
    sync::{Notify, watch},
    task::JoinHandle,
};

#[derive(Clone, Default)]
struct Progress {
    initialized: bool,
    applied: u64,
    error: Option<Arc<anyhow::Error>>,
    finished: bool,
    /// Test-only instrumentation: canonical commits folded into the resident projection, a
    /// strictly monotonic proof that each commit is applied exactly once without a prefix replay.
    #[cfg(test)]
    applies: u64,
}
struct WorkerState {
    progress: watch::Sender<Progress>,
    retry: Notify,
}
struct Observation {
    thread: ThreadHandle,
    state: Arc<WorkerState>,
    task: Mutex<Option<JoinHandle<()>>>,
}
impl Drop for Observation {
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
#[derive(Clone)]
pub(super) struct ObservationServices {
    pub(super) store: StudioStore,
    pub(super) events: ProductEventBus,
    pub(super) performance: ModelPerformanceOwner,
    pub(super) threads: crate::thread_assembler::StudioThreadAssembler,
    pub(super) writer: crate::studio::agent_host::ThreadWriteBehindWriter,
}
struct Shared {
    projector: ObservationServices,
    observations: Mutex<BTreeMap<String, Vec<Arc<Observation>>>>,
}
#[derive(Clone)]
pub(super) struct ThreadObservations(Arc<Shared>);

impl ThreadObservations {
    pub(super) fn new(projector: ObservationServices) -> Self {
        Self(Arc::new(Shared {
            projector,
            observations: Mutex::new(BTreeMap::new()),
        }))
    }

    pub(super) fn install(
        &self,
        thread_factory: crate::studio::thread_factory::StudioThreadFactory,
    ) -> Result<()> {
        let owner = Arc::downgrade(&self.0);
        let draining = owner.clone();
        self.0.projector.threads.observe_assembly(
            move |id, thread| {
                if let Some(owner) = owner.upgrade() {
                    ThreadObservations(owner).observe(id, thread);
                }
            },
            move |id| {
                let owner = draining.clone();
                let thread_factory = thread_factory.clone();
                Box::pin(async move {
                    let owner = owner
                        .upgrade()
                        .ok_or(crate::thread_assembler::ThreadAssemblyError::Closed)?;
                    let observations = ThreadObservations(owner);
                    observations
                        .synchronize(Some(&id))
                        .await
                        .map_err(|source| {
                            crate::thread_assembler::ThreadAssemblyError::Resource {
                                operation: "drain Thread product observation",
                                source: source.into_boxed_dyn_error(),
                            }
                        })?;
                    thread_factory.forget_tool_binding(&id);
                    Ok(())
                })
            },
        )?;
        Ok(())
    }

    fn observe(&self, id: String, thread: ThreadHandle) {
        let (progress, _) = watch::channel(Progress::default());
        let state = Arc::new(WorkerState {
            progress,
            retry: Notify::new(),
        });
        let worker_state = state.clone();
        let projector = self.0.projector.clone();
        let worker_thread = thread.clone();
        let worker_id = id.clone();
        let recovered_through = thread.snapshot().commit_sequence;
        let task = tokio::spawn(async move {
            run(
                projector,
                worker_id,
                worker_thread,
                &worker_state,
                recovered_through,
            )
            .await;
        });
        let observation = Arc::new(Observation {
            thread,
            state,
            task: Mutex::new(Some(task)),
        });
        let mut all = self
            .0
            .observations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let previous = all.entry(id).or_default();
        previous.retain(|entry| {
            let progress = entry.state.progress.borrow();
            !progress.finished || progress.error.is_some()
        });
        previous.push(observation);
    }

    async fn synchronize(&self, id: Option<&str>) -> Result<()> {
        self.0.projector.writer.retry_now();
        let observations: Vec<_> = self
            .0
            .observations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .filter(|(key, _)| id.is_none_or(|id| id == key.as_str()))
            .flat_map(|(id, entries)| entries.iter().map(|entry| (id.clone(), entry.clone())))
            .collect();
        for (id, observation) in observations {
            let target_snapshot = observation.thread.snapshot();
            let target = target_snapshot.commit_sequence;
            let mut progress = observation.state.progress.subscribe();
            observation.state.progress.send_modify(|progress| {
                if !progress.finished {
                    progress.error = None;
                }
            });
            observation.state.retry.notify_one();
            loop {
                let state = progress.borrow().clone();
                if let Some(error) = state.error {
                    bail!("Thread {id} product observation failed: {error:#}");
                }
                if state.initialized
                    && state.applied >= target
                    && (target_snapshot.lifecycle != ThreadLifecycle::Closed || state.finished)
                {
                    break;
                }
                if state.finished {
                    bail!("Thread {id} product observer ended before commit {target}");
                }
                progress
                    .changed()
                    .await
                    .context("Thread product observer progress closed")?;
            }
        }
        Ok(())
    }

    /// Explicit parent continuation repairs terminal notifications even for unloaded children.
    /// This reads saved journals only; no child model or workspace is opened.
    pub(super) async fn reconcile_children(&self, parent: &str) -> Result<()> {
        let services = &self.0.projector;
        for child in services.store.list_threads_for_root(parent).await? {
            if child.parent_thread_id.as_deref() != Some(parent) {
                continue;
            }
            if services.threads.thread(&child.id).is_some() {
                self.synchronize(Some(&child.id)).await?;
                continue;
            }
            let history = services
                .store
                .sessions()
                .read_thread_journal(&child.id)
                .await?;
            let child = pl_protocol::Thread::from(child);
            // Fold the saved journal once through the same incremental projection the live path
            // uses, so every already-terminal Turn is repaired exactly once without replaying a
            // prefix per commit.
            let snapshot = pl_core::thread::journal::replay(&history)?;
            let mut state =
                ProjectionState::new(child.id.as_str(), child.parent_thread_id.as_deref());
            let mut tracker = ReportTracker::default();
            for commit in &history {
                state.apply(commit)?;
                tracker
                    .observe(services, &child, &state, &snapshot, commit, false)
                    .await?;
            }
        }
        Ok(())
    }

    pub(super) async fn finish(&self) -> Result<()> {
        self.synchronize(None).await?;
        let observations: Vec<_> = self
            .0
            .observations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
            .flatten()
            .cloned()
            .collect();
        for observation in observations {
            let mut progress = observation.state.progress.subscribe();
            loop {
                let state = progress.borrow().clone();
                if let Some(error) = state.error {
                    bail!("Thread product observer failed: {error:#}");
                }
                if state.finished {
                    break;
                }
                progress
                    .changed()
                    .await
                    .context("Thread product observer ended without final state")?;
            }
            let task = observation
                .task
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            if let Some(task) = task {
                task.await.context("Thread product observer join failed")?;
            }
        }
        Ok(())
    }
}

impl StudioRuntime {
    /// Waits for product directories and billing to reach the current committed Thread watermark.
    /// Reattempts a previously failed projection without invoking models or tools.
    pub async fn synchronize_thread_observation(&self, thread_id: &str) -> Result<()> {
        self.thread_observations.synchronize(Some(thread_id)).await
    }
}

async fn run(
    projector: ObservationServices,
    id: String,
    thread: ThreadHandle,
    state: &WorkerState,
    recovered_through: u64,
) {
    let mut updates = thread.subscribe();
    let mut projection: Option<ThreadProjection> = None;
    let mut persistence = projector.store.sessions().subscribe_persistence();
    let mut current = thread.snapshot();
    let mut projected: Option<(u64, ThreadLifecycle)> = None;
    loop {
        use futures::FutureExt;
        let result = if projected == Some((current.commit_sequence, current.lifecycle)) {
            Ok(true)
        } else {
            std::panic::AssertUnwindSafe(project(
                &projector,
                &id,
                &thread,
                &current,
                &mut projection,
                recovered_through,
            ))
            .catch_unwind()
            .await
            .unwrap_or_else(|_| Err(anyhow::anyhow!("Thread product projection panicked")))
        };
        let watermark = projection.as_ref().map_or(0, ThreadProjection::watermark);
        match result {
            Ok(reached) => {
                // Only a snapshot every observed commit is projected holds the guard; a projection
                // still waiting on durability stays unguarded so the next wake resumes it.
                projected = reached.then_some((current.commit_sequence, current.lifecycle));
                state.progress.send_replace(Progress {
                    initialized: true,
                    applied: watermark,
                    error: None,
                    finished: false,
                    #[cfg(test)]
                    applies: projection.as_ref().map_or(0, ThreadProjection::applies),
                });
                if current.lifecycle == ThreadLifecycle::Closed
                    && watermark >= current.commit_sequence
                {
                    match projector.writer.flush().await {
                        Ok(()) => {
                            state
                                .progress
                                .send_modify(|progress| progress.finished = true);
                            return;
                        }
                        Err(error) => state.progress.send_modify(|progress| {
                            progress.error = Some(Arc::new(anyhow::Error::new(error)));
                            progress.finished = false;
                        }),
                    }
                }
            }
            Err(error) => {
                projected = None;
                state
                    .progress
                    .send_modify(|progress| progress.error = Some(Arc::new(error)));
            }
        }
        if current.lifecycle == ThreadLifecycle::Closed {
        tokio::select! {
                () = state.retry.notified() => {}
                () = session_durable_wake(&mut persistence) => {}
                }
            current = thread.snapshot();
            continue;
            }
        tokio::select! {
            () = state.retry.notified() => current = thread.snapshot(),
            () = session_durable_wake(&mut persistence) => current = thread.snapshot(),
            update = updates.next() => match update {
                Some(snapshot) => current = snapshot,
                None => {
                    state.progress.send_modify(|progress| progress.error = Some(Arc::new(anyhow::anyhow!("Thread subscription ended before its final projection"))));
                    state.retry.notified().await;
                    current = thread.snapshot();
        }
    }
}
    }
}

async fn project(
    projector: &ObservationServices,
    id: &str,
    thread: &ThreadHandle,
    snapshot: &ThreadSnapshot,
    projection: &mut Option<ThreadProjection>,
    recovered_through: u64,
) -> Result<bool> {
    let mut product = match projector.events.thread_snapshot(id) {
        Some(thread) => thread,
        None => {
            // An archived hot entry may disappear before its original registration reaches SQLite.
            // Drain the accepted directory writes before resolving the cold association.
            projector.writer.flush().await?;
            pl_protocol::Thread::from(
                projector
                    .store
                    .read_thread_association(id)
                    .await?
                    .context("observed Thread has no product association")?,
            )
        }
    };
    if projection.is_none() {
        *projection = Some(
            ThreadProjection::open(projector, id, product.parent_thread_id.as_deref()).await?,
        );
    }
    let projection = projection.as_mut().expect("projection was just initialised");
    projection
        .advance(projector, id, thread, snapshot, &product, recovered_through)
        .await?;
    let reached = projection.watermark() >= snapshot.commit_sequence;
    if !reached {
        return Ok(false);
}
    product.status = crate::studio::thread_projection::status(snapshot);
    product.updated_at = product
        .updated_at
        .max(projection.last_committed_at());
    directory::publish(
        projector,
        product,
        snapshot,
        &projection.state,
        &projection.last_delta,
    )
    .await?;
    Ok(true)
}

/// Resolves when the aggregated per-Thread session writer publishes new durability progress, and
/// never resolves once its sender is gone (the owner then tears the observation worker down).
async fn session_durable_wake(
    receiver: &mut watch::Receiver<pl_core::persistence::SessionPersistenceSnapshot>,
) {
    if receiver.changed().await.is_err() {
        std::future::pending::<()>().await;
    }
    }

/// Reads exactly one immutable commit from the owner's journal at a fixed watermark.
async fn fetch_commit(thread: &ThreadHandle, id: &str, sequence: u64) -> Result<Arc<ThreadCommit>> {
    let page = thread
        .journal_page(
            sequence.saturating_sub(1),
            NonZeroUsize::new(1).expect("constant is nonzero"),
        )
                    .await?;
    page.into_iter()
        .next()
        .filter(|commit| commit.sequence == sequence)
        .with_context(|| format!("Thread {id} journal did not reach observed watermark {sequence}"))
            }

/// One Thread's durable Studio timeline index inside its own session database.
///
/// The core journal reader proves which commits are already durable in the same database, so the
/// derived index can never lead the facts it derives from; the reader loads only the working set a
/// commit needs, and the writer commits head, facts, slots and panel in one transaction.
struct TimelineIndex {
    journal: pl_core::persistence::SessionJournalReader,
    reader: crate::studio::timeline_store::TimelineReader,
    writer: crate::studio::timeline_store::TimelineWriter,
        }

impl TimelineIndex {
    async fn open(path: std::path::PathBuf, id: &str) -> Result<Self> {
        let journal =
            pl_core::persistence::open_journal_reader(pl_core::persistence::SqliteSessionOptions {
                path: path.clone(),
            })
            .await
            .map_err(|error| anyhow::anyhow!("open Thread {id} journal reader: {error}"))?;
        // Opening the writer first provisions the derived schema, so the reader never treats an
        // un-indexed session database as an empty timeline.
        let writer = crate::studio::timeline_store::TimelineWriter::open(&path).await?;
        let reader = crate::studio::timeline_store::TimelineReader::open(&path).await?;
        Ok(Self {
            journal,
            reader,
            writer,
        })
    }

    /// True when the durable journal in the same session database already holds `sequence`.
    async fn is_durable(&self, id: &str, sequence: u64) -> Result<bool> {
        let page = self
            .journal
            .read_page(
                id,
                sequence.saturating_sub(1),
                NonZeroUsize::new(1).expect("constant is nonzero"),
            )
            .await
            .map_err(|error| anyhow::anyhow!("read Thread {id} durable journal: {error}"))?;
        Ok(page.first().map(|commit| commit.sequence) == Some(sequence))
}
}

/// Per-commit product consumption shared by the live driver and the explicit repair path.
///
/// Billing is idempotent per commit, and each terminal Turn notifies its parent exactly once
/// through the stable message identity derived from the commit sequence.
#[derive(Default)]
struct ReportTracker {
    reported_turns: BTreeSet<String>,
    turn_open: BTreeMap<String, (u64, u64)>,
    running_consumed: u64,
    running_wake: u64,
}

impl ReportTracker {
    async fn observe(
        &mut self,
        services: &ObservationServices,
        product: &pl_protocol::Thread,
        state: &ProjectionState,
        snapshot: &ThreadSnapshot,
        commit: &ThreadCommit,
        wake: bool,
    ) -> Result<()> {
        billing::record(&services.performance, &product.root_thread_id, commit)?;
        if let Some(turn) = &commit.turn
            && !self.turn_open.contains_key(&turn.turn_id)
        {
            self.turn_open.insert(
                turn.turn_id.clone(),
                (self.running_consumed, self.running_wake),
            );
        }
        if let Some(consumed) = commit.consumed_messages {
            self.running_consumed = self.running_consumed.max(consumed);
        }
        if let Some(through) = commit.wake_messages_through {
            self.running_wake = self.running_wake.max(through);
        }
        // A terminal Turn reports once, through the first commit that reaches the terminal state.
        let Some(turn) = commit
            .turn
            .as_ref()
            .filter(|turn| turn.state != TurnState::Running)
        else {
            return Ok(());
        };
        if !self.reported_turns.insert(turn.turn_id.clone()) {
            return Ok(());
        }
        let opening = self
            .turn_open
            .get(&turn.turn_id)
            .copied()
            .unwrap_or((0, 0));
        reports::publish(
            services,
            product,
            state,
            snapshot,
            commit,
            opening,
            self.running_consumed,
            wake,
        )
        .await
    }
}

/// One Thread's single incremental product projection.
///
/// A resident [`ProjectionState`] is advanced exactly once per canonical commit. The resulting
/// [`ProjectionDelta`] persists the durable timeline index (when the Thread has a session database)
/// and drives the product directory, billing and parent notifications, so the index write and the
/// product projection always share one state and one watermark (design/17 §17.2, design/18 §18.8).
struct ThreadProjection {
    state: ProjectionState,
    index: Option<TimelineIndex>,
    tracker: ReportTracker,
    #[cfg(test)]
    applies: u64,
    last_committed_at: i64,
    last_delta: ProjectionDelta,
}

impl ThreadProjection {
    async fn open(
        projector: &ObservationServices,
        id: &str,
        parent_id: Option<&str>,
    ) -> Result<Self> {
        let index = match projector.store.sessions().sessions_dir() {
            Some(dir) => {
                let path = crate::studio::paths::session_database_path(dir, id)?;
                if tokio::fs::try_exists(&path).await? {
                    Some(TimelineIndex::open(path, id).await?)
                } else {
                    None
                }
            }
            None => None,
        };
        let state = match &index {
            Some(index) => match index.reader.read_head(id).await {
                Ok(head) => {
                    let panel = index.reader.read_panel(id).await?;
                    // A restored head keeps the bounded working set: the facts and slots for each
                    // commit are loaded from the index, never a whole decoded history.
                    let facts = Facts {
                        message_source: parent_id.map(|parent| format!("agent:{parent}")),
                        ..Default::default()
                    };
                    ProjectionState::restore(head, facts, panel, Vec::new())
                }
                Err(crate::studio::timeline_store::TimelineStoreError::ThreadNotIndexed {
                    ..
                }) => ProjectionState::new(id, parent_id),
                Err(error) => return Err(error.into()),
            },
            None => ProjectionState::new(id, parent_id),
        };
        Ok(Self {
            state,
            index,
            tracker: ReportTracker::default(),
            #[cfg(test)]
            applies: 0,
            last_committed_at: 0,
            last_delta: ProjectionDelta::default(),
        })
    }

    /// Folds every commit the resident projection is missing, in order, at the observed watermark.
    ///
    /// Each canonical commit is applied exactly once. A commit enters the durable index only once
    /// its core row is durable in the same session database; while durability lags the observed
    /// watermark the projection stops and the worker resumes it on the next durability wake, so no
    /// history prefix is ever replayed.
    async fn advance(
        &mut self,
        projector: &ObservationServices,
        id: &str,
        thread: &ThreadHandle,
        snapshot: &ThreadSnapshot,
        product: &pl_protocol::Thread,
        recovered_through: u64,
    ) -> Result<()> {
        while self.state.watermark() < snapshot.commit_sequence {
            let sequence = self.state.watermark() + 1;
            let commit = fetch_commit(thread, id, sequence).await?;
            if let Some(index) = &self.index {
                if !index.is_durable(id, sequence).await? {
                    break;
                }
                // Two-phase plan: requirements are fact-independent, while `slot_keys` also names
                // slots the commit consumes, which the projector reads from the loaded facts.
                let requirements = self.state.apply_plan(&commit).requirements;
                let facts = index.reader.load_requirements(id, &requirements).await?;
                self.state.load_facts(facts.rows)?;
                let slot_keys = self.state.apply_plan(&commit).slot_keys;
                let slots = index.reader.read_slots(id, &slot_keys).await?;
                self.state.load_slots(slots)?;
            }
            let delta = self.state.apply(&commit)?;
            if let Some(index) = &self.index {
                index
                    .writer
                    .persist_commit(&commit, &self.state, &delta)
                    .await?;
            }
            #[cfg(test)]
            {
            self.applies += 1;
            }
            self.last_committed_at = self.last_committed_at.max(commit.committed_at);
            self.last_delta = delta;
            self.tracker
                .observe(
                    projector,
                    product,
                    &self.state,
                    snapshot,
                    &commit,
                    commit.sequence > recovered_through,
                )
                .await?;
        }
        Ok(())
    }

    fn watermark(&self) -> u64 {
        self.state.watermark()
    }

    #[cfg(test)]
    fn applies(&self) -> u64 {
        self.applies
    }

    fn last_committed_at(&self) -> i64 {
        self.last_committed_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_core::{
        context::ContextContent,
        model::{
            DynModelSession, ModelError, ModelRequest, ModelSession, ModelStepOutput,
            PreparedModelCall,
        },
    };
    use pretty_assertions::assert_eq;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Reply(Arc<AtomicUsize>);
    impl ModelSession for Reply {
        async fn prepare(
            &mut self,
            request: ModelRequest,
        ) -> Result<PreparedModelCall, ModelError> {
            let calls = self.0.clone();
            Ok(PreparedModelCall::new(async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(ModelStepOutput {
                    attempt_id: request.attempt_id,
                    base_context_revision: request.context.revision,
                    content: vec![ContextContent::Text {
                        text: Arc::from("saved reply"),
                    }],
                    tool_calls: Vec::new(),
                    private_context: None,
                    usage: pl_core::model::ModelUsage {
                        input_tokens: Some(11),
                        output_tokens: Some(7),
                        ..Default::default()
                    },
                })
            }))
        }
        async fn close(&mut self) -> Result<(), ModelError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn failed_directory_projection_retries_without_model_replay_and_closes_after_billing_save()
     {
        let store = StudioStore::open_memory().await.unwrap();
        let writer = crate::studio::agent_host::ThreadWriteBehindWriter::new(store.clone());
        let events = ProductEventBus::new(store.clone(), writer.clone());
        let performance = ModelPerformanceOwner::new(store.clone(), writer.clone(), events.clone());
        let observers = ThreadObservations::new(ObservationServices {
            store: store.clone(),
            events: events.clone(),
            performance: performance.clone(),
            threads: crate::thread_assembler::StudioThreadAssembler::default(),
            writer: writer.clone(),
        });
        let workspace = tempfile::tempdir().unwrap();
        let project = store.upsert_project(workspace.path()).await.unwrap();
        let (_, product) = crate::studio::store::directory::DirectoryDelta::register_root_thread(
            crate::studio::ids::new_id("thread"),
            &project.id,
            "task",
            pl_protocol::ThreadModeId::simple(),
            pl_protocol::ThreadWorkspaceMode::Local,
            project.path.clone(),
        );
        let calls = Arc::new(AtomicUsize::new(0));
        let thread = ThreadHandle::start(
            product.id.clone(),
            DynModelSession::new(Reply(calls.clone())),
        )
        .unwrap();
        observers.observe(product.id.clone(), thread.clone());
        let observation = observers
            .0
            .observations
            .lock()
            .unwrap()
            .get(&product.id)
            .unwrap()[0]
            .clone();
        let mut progress = observation.state.progress.subscribe();
        while progress.borrow().error.is_none() {
            progress.changed().await.unwrap();
        }
        assert_eq!(progress.borrow().initialized, false);
        events
            .apply_thread_delta(vec![product.clone()], Vec::new())
            .await
            .unwrap();
        observers.synchronize(Some(&product.id)).await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        thread
            .step(pl_core::thread::StepInput {
                turn_id: "turn".into(),
                attempt_id: "attempt".into(),
                content: vec![ContextContent::Text {
                    text: Arc::from("question"),
                }],
                cancellation: tokio_util::sync::CancellationToken::new(),
            })
            .await
            .unwrap();
        observers.synchronize(Some(&product.id)).await.unwrap();
        let observed = thread.snapshot().commit_sequence;
        assert_eq!(progress.borrow().applied, observed);
        assert_eq!(progress.borrow().applies, observed);
        let billed = performance.snapshot().await;
        assert_eq!(billed.revision, 1);
        observers.synchronize(Some(&product.id)).await.unwrap();
        assert_eq!(performance.snapshot().await, billed);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        thread.close().await.unwrap();
        observers.finish().await.unwrap();
        assert_eq!(
            events.thread_snapshot(&product.id).unwrap().status,
            pl_protocol::ThreadStatus::Closed
        );
        assert_eq!(writer.pending_commit_count(), 0);
        let reloaded = ModelPerformanceOwner::new(store.clone(), writer.clone(), events);
        reloaded.load_cache().await.unwrap();
        assert_eq!(reloaded.snapshot().await, billed);
        writer.shutdown().await.unwrap();
        store.sessions().shutdown().await.unwrap();
    }

    /// One real temp-SQLite Studio store with a resident, cold-attached root Thread.
    struct ObservationHarness {
        home: tempfile::TempDir,
        _workspace: tempfile::TempDir,
        store: StudioStore,
        writer: crate::studio::agent_host::ThreadWriteBehindWriter,
        events: ProductEventBus,
        observers: ThreadObservations,
        thread: ThreadHandle,
        product: pl_protocol::Thread,
}

    impl ObservationHarness {
        async fn start(model: DynModelSession, register: bool) -> Self {
            let home = tempfile::tempdir().unwrap();
            let store = StudioStore::open(home.path().join("studio.sqlite"))
                .await
                .unwrap();
            let writer = crate::studio::agent_host::ThreadWriteBehindWriter::new(store.clone());
            let events = ProductEventBus::new(store.clone(), writer.clone());
            let performance =
                ModelPerformanceOwner::new(store.clone(), writer.clone(), events.clone());
            let observers = ThreadObservations::new(ObservationServices {
                store: store.clone(),
                events: events.clone(),
                performance,
                threads: crate::thread_assembler::StudioThreadAssembler::default(),
                writer: writer.clone(),
            });
            let workspace = tempfile::tempdir().unwrap();
            let project = store.upsert_project(workspace.path()).await.unwrap();
            let (delta, product) =
                crate::studio::store::directory::DirectoryDelta::register_root_thread(
                    crate::studio::ids::new_id("thread"),
                    &project.id,
                    "task",
                    pl_protocol::ThreadModeId::simple(),
                    pl_protocol::ThreadWorkspaceMode::Local,
                    project.path.clone(),
                );
            let thread = ThreadHandle::start(product.id.clone(), model).unwrap();
            thread
                .attach_storage(pl_core::thread::cold::ColdStoreHandle::new(
                    store.sessions().open_thread(&product.id).await.unwrap(),
                ))
                .await
                .unwrap();
            if register {
                events.commit_directory(delta).await.unwrap();
                writer.flush().await.unwrap();
            }
            observers.observe(product.id.clone(), thread.clone());
            Self {
                home,
                _workspace: workspace,
                store,
                writer,
                events,
                observers,
                thread,
                product,
            }
        }

        fn observation(&self) -> Arc<Observation> {
            self.observers
                .0
                .observations
                .lock()
                .unwrap()
                .get(&self.product.id)
                .unwrap()[0]
                .clone()
        }

        fn progress(&self) -> Progress {
            self.observation().state.progress.borrow().clone()
        }

        async fn wait_for_error(&self) {
            let mut progress = self.observation().state.progress.subscribe();
            while progress.borrow().error.is_none() {
                progress.changed().await.unwrap();
            }
        }

        fn index_path(&self) -> std::path::PathBuf {
            crate::studio::paths::session_database_path(
                self.store.sessions().sessions_dir().expect("file-backed"),
                &self.product.id,
            )
            .unwrap()
        }

        async fn step(&self, turn: &str) {
            self.thread
                .step(pl_core::thread::StepInput {
                    turn_id: turn.into(),
                    attempt_id: format!("{turn}-attempt"),
                    content: vec![ContextContent::Text {
                        text: Arc::from("question"),
                    }],
                    cancellation: tokio_util::sync::CancellationToken::new(),
                })
                .await
                .unwrap();
        }

        async fn synchronize(&self) {
            self.observers
                .synchronize(Some(&self.product.id))
                .await
                .unwrap();
        }

        async fn close(&self) {
            self.thread.close().await.unwrap();
            self.observers.finish().await.unwrap();
            self.writer.flush().await.unwrap();
        }
    }

    #[tokio::test]
    async fn one_incremental_projection_advances_the_index_and_the_cold_directory() {
        let calls = Arc::new(AtomicUsize::new(0));
        let harness =
            ObservationHarness::start(DynModelSession::new(Reply(calls.clone())), true).await;
        harness.step("turn-1").await;
        harness.step("turn-2").await;
        let observed = harness.thread.snapshot().commit_sequence;
        assert!(observed >= 2, "expected several canonical commits");
        harness.synchronize().await;

        let progress = harness.progress();
        assert!(progress.initialized);
        assert!(progress.error.is_none());
        // Every canonical commit is folded exactly once: the projection watermark and the applied
        // count both equal the observed journal length, so no history prefix was replayed.
        assert_eq!(progress.applied, observed);
        assert_eq!(progress.applies, observed);

        // The durable timeline index writes the same state at the same watermark.
        let reader = crate::studio::timeline_store::TimelineReader::open(harness.index_path())
            .await
            .unwrap();
        let head = reader.read_head(&harness.product.id).await.unwrap();
        assert_eq!(head.watermark, progress.applied);
        reader.close().await.unwrap();

        // Re-synchronizing an unchanged snapshot applies nothing a second time.
        harness.synchronize().await;
        assert_eq!(harness.progress().applies, observed);
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        harness.close().await;

        // The durable directory summary carries the observed status without a journal replay.
        let durable = harness
            .store
            .read_thread_association(&harness.product.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(durable.status, pl_protocol::ThreadStatus::Closed);
        assert!(durable.updated_at >= harness.product.updated_at);

        // Restarting over the same database keeps the cold directory facts without opening a
        // journal.
        harness.store.sessions().shutdown().await.unwrap();
        harness.writer.shutdown().await.unwrap();
        let reopened = StudioStore::open(harness.home.path().join("studio.sqlite"))
            .await
            .unwrap();
        let cold = reopened
            .read_thread_association(&harness.product.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(cold.status, pl_protocol::ThreadStatus::Closed);
        assert_eq!(cold.updated_at, durable.updated_at);
        reopened.sessions().shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn a_failed_projection_is_retried_and_only_ever_applies_each_commit_once() {
        let calls = Arc::new(AtomicUsize::new(0));
        let harness =
            ObservationHarness::start(DynModelSession::new(Reply(calls.clone())), false).await;
        harness.step("turn-1").await;
        // The observation fails: the Thread has a durable journal but no product association yet.
        harness.wait_for_error().await;
        assert!(!harness.progress().initialized);
        assert_eq!(harness.progress().applies, 0);

        // Registering the association lets the same worker retry and fold the whole journal once.
        harness
            .events
            .commit_directory(crate::studio::store::directory::DirectoryDelta {
                thread_upserts: vec![harness.product.clone()],
                ..Default::default()
            })
            .await
            .unwrap();
        harness.synchronize().await;
        let observed = harness.thread.snapshot().commit_sequence;
        assert_eq!(harness.progress().applied, observed);
        assert_eq!(harness.progress().applies, observed);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        harness.close().await;

        harness.store.sessions().shutdown().await.unwrap();
        harness.writer.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn a_late_observation_never_overwrites_archived_or_renamed_directory_facts() {
        let calls = Arc::new(AtomicUsize::new(0));
        let harness =
            ObservationHarness::start(DynModelSession::new(Reply(calls.clone())), true).await;
        harness.step("turn-1").await;
        harness.synchronize().await;

        // Rename and archive are directory owner commands; both are durable facts.
        let mut renamed = harness.events.thread_snapshot(&harness.product.id).unwrap();
        renamed.title = "renamed task".into();
        renamed.updated_at = crate::studio::ids::unix_seconds();
        harness
            .events
            .commit_directory(crate::studio::store::directory::DirectoryDelta {
                thread_upserts: vec![renamed.clone()],
                ..Default::default()
            })
            .await
            .unwrap();
        harness
            .events
            .commit_directory(
                crate::studio::store::directory::DirectoryDelta::archive_threads(vec![
                    harness.product.id.clone(),
                ]),
            )
            .await
            .unwrap();
        harness.writer.flush().await.unwrap();

        // A late observation of a newer commit is rejected by the directory owner lock and never
        // re-inserts the archived entry nor rewrites the renamed facts.
        harness.step("turn-2").await;
        harness.synchronize().await;
        harness.writer.flush().await.unwrap();

        let durable = harness
            .store
            .read_thread_association(&harness.product.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(durable.title, "renamed task");
        assert_eq!(
            durable.visibility,
            crate::studio::records::ThreadVisibility::Archived
        );
        assert!(
            harness
                .events
                .thread_snapshot(&harness.product.id)
                .is_none_or(|thread| thread.archived)
        );

        harness.store.sessions().shutdown().await.unwrap();
        harness.writer.shutdown().await.unwrap();
    }
}
