//! Owned, retryable product projections of the immutable Thread journal.
mod billing;
mod directory;

use super::{ModelPerformanceOwner, StudioRuntime};
use crate::studio::{ProductEventBus, StudioStore};
use anyhow::{Context, Result, bail};
use pl_core::thread::{ThreadHandle, ThreadLifecycle, ThreadSnapshot, journal::ThreadCommit};
use std::{
    collections::BTreeMap,
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
        let baseline = thread.snapshot().commit_sequence;
        let (progress, _) = watch::channel(Progress::default());
        let state = Arc::new(WorkerState {
            progress,
            retry: Notify::new(),
        });
        let worker_state = state.clone();
        let projector = self.0.projector.clone();
        let worker_thread = thread.clone();
        let worker_id = id.clone();
        let task = tokio::spawn(async move {
            run(projector, worker_id, worker_thread, baseline, &worker_state).await;
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
    baseline: u64,
    state: &WorkerState,
) {
    let mut updates = thread.subscribe();
    let mut journal = Vec::<Arc<ThreadCommit>>::new();
    let mut current = thread.snapshot();
    let mut projected = None;
    loop {
        use futures::FutureExt;
        let result = if projected == Some((current.commit_sequence, current.lifecycle)) {
            Ok(())
        } else {
            std::panic::AssertUnwindSafe(project(
                &projector,
                &id,
                &thread,
                &current,
                baseline,
                &mut journal,
                state,
            ))
            .catch_unwind()
            .await
            .unwrap_or_else(|_| Err(anyhow::anyhow!("Thread product projection panicked")))
        };
        match result {
            Ok(()) => {
                projected = Some((current.commit_sequence, current.lifecycle));
                let finished = current.lifecycle == ThreadLifecycle::Closed;
                state.progress.send_replace(Progress {
                    initialized: true,
                    applied: current.commit_sequence,
                    error: None,
                    finished: false,
                });
                if finished {
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
                state
                    .progress
                    .send_modify(|progress| progress.error = Some(Arc::new(error)));
            }
        }
        if current.lifecycle == ThreadLifecycle::Closed {
            state.retry.notified().await;
            current = thread.snapshot();
            continue;
        }
        tokio::select! {
            () = state.retry.notified() => current = thread.snapshot(),
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
    baseline: u64,
    journal: &mut Vec<Arc<ThreadCommit>>,
    state: &WorkerState,
) -> Result<()> {
    while (journal.len() as u64) < snapshot.commit_sequence {
        let page = thread
            .journal_page(
                journal.len() as u64,
                NonZeroUsize::new(128).expect("constant is nonzero"),
            )
            .await?;
        let before = journal.len();
        journal.extend(
            page.into_iter()
                .take_while(|commit| commit.sequence <= snapshot.commit_sequence),
        );
        if before == journal.len() {
            bail!("Thread {id} journal did not reach observed watermark");
        }
    }
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
    let applied = state.progress.borrow().applied;
    for commit in journal.iter().filter(|commit| commit.sequence > applied) {
        billing::record(&projector.performance, &product.root_thread_id, commit)?;
        if commit.sequence > baseline {
            directory::notify_parent(projector, &product, commit).await?;
        }
    }
    product.status = crate::studio::thread_projection::status(snapshot);
    product.updated_at = product
        .updated_at
        .max(journal.last().map_or(0, |commit| commit.committed_at));
    directory::publish(projector, product, snapshot).await
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
            &project.id,
            "task",
            pl_protocol::ThreadModeId::simple(),
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
}
