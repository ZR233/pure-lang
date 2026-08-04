use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use pl_core::{AgentCommitOutcome, AgentStateMutation, SessionHistoryCommit};
use tokio::sync::{Mutex, RwLock, mpsc, oneshot};

use crate::PureError;
use crate::studio::StudioStore;
use crate::studio::store::history::persistence::persist_history_batch;

const PERSISTENCE_QUEUE_CAPACITY: usize = 1024;
const MAX_COMMIT_BATCH: usize = 128;

#[derive(Clone)]
pub(super) struct AgentPersistenceWriter {
    sender: mpsc::Sender<WriterCommand>,
    acceptance: Arc<Mutex<AcceptanceState>>,
    failure: Arc<RwLock<Option<String>>>,
    durable_watermark: Arc<AtomicU64>,
}

#[derive(Default)]
struct AcceptanceState {
    revisions: HashMap<String, Option<u64>>,
    watermark: u64,
}

impl AgentPersistenceWriter {
    pub(super) fn spawn(store: StudioStore) -> Self {
        let (sender, receiver) = mpsc::channel(PERSISTENCE_QUEUE_CAPACITY);
        let writer = Self {
            sender,
            acceptance: Arc::new(Mutex::new(AcceptanceState::default())),
            failure: Arc::new(RwLock::new(None)),
            durable_watermark: Arc::new(AtomicU64::new(0)),
        };
        tokio::spawn(run_writer(
            store,
            receiver,
            Arc::clone(&writer.failure),
            Arc::clone(&writer.durable_watermark),
        ));
        writer
    }

    pub(super) async fn seed_revision(&self, agent_id: &str, revision: u64) {
        self.acceptance
            .lock()
            .await
            .revisions
            .insert(agent_id.to_string(), Some(revision));
    }

    pub(super) async fn submit(
        &self,
        commit: SessionHistoryCommit,
    ) -> Result<AgentCommitOutcome, PureError> {
        self.ensure_healthy().await?;
        let deferred = is_deferred_checkpoint(&commit);
        let (ack_sender, ack_receiver) = if deferred {
            (None, None)
        } else {
            let (sender, receiver) = oneshot::channel();
            (Some(sender), Some(receiver))
        };
        let agent_id = commit.agent_id.to_string();
        let next_revision = commit.next_state.snapshot.revision;
        let mut acceptance = self.acceptance.lock().await;
        let actual_revision = acceptance.revisions.get(&agent_id).copied().flatten();
        if actual_revision != commit.expected_revision {
            return Ok(AgentCommitOutcome::RevisionConflict { actual_revision });
        }
        let request_id = acceptance
            .watermark
            .checked_add(1)
            .ok_or_else(|| writer_error("persistence request watermark overflowed"))?;
        self.sender
            .try_send(WriterCommand::Commit(Box::new(QueuedCommit {
                request_id,
                commit,
                ack: ack_sender,
            })))
            .map_err(queue_error)?;
        acceptance.watermark = request_id;
        acceptance.revisions.insert(agent_id, Some(next_revision));
        drop(acceptance);

        let Some(ack_receiver) = ack_receiver else {
            tracing::trace!(request_id, "queued deferred session checkpoint");
            return Ok(AgentCommitOutcome::Applied);
        };
        ack_receiver
            .await
            .map_err(|_| writer_error("persistence writer stopped before acknowledging commit"))?
            .map_err(writer_error)
    }

    pub(super) async fn barrier(&self) -> Result<(), PureError> {
        self.ensure_healthy().await?;
        let target = self.acceptance.lock().await.watermark;
        if self.durable_watermark.load(Ordering::Acquire) >= target {
            return Ok(());
        }
        let (ack, receiver) = oneshot::channel();
        self.sender
            .try_send(WriterCommand::Barrier { target, ack })
            .map_err(queue_error)?;
        receiver
            .await
            .map_err(|_| writer_error("persistence writer stopped before barrier"))?
            .map_err(writer_error)
    }

    async fn ensure_healthy(&self) -> Result<(), PureError> {
        if let Some(error) = self.failure.read().await.as_ref() {
            return Err(writer_error(format!(
                "persistence writer is faulted: {error}"
            )));
        }
        Ok(())
    }
}

struct QueuedCommit {
    request_id: u64,
    commit: SessionHistoryCommit,
    ack: Option<oneshot::Sender<Result<AgentCommitOutcome, String>>>,
}

enum WriterCommand {
    Commit(Box<QueuedCommit>),
    Barrier {
        target: u64,
        ack: oneshot::Sender<Result<(), String>>,
    },
}

async fn run_writer(
    store: StudioStore,
    mut receiver: mpsc::Receiver<WriterCommand>,
    failure: Arc<RwLock<Option<String>>>,
    durable_watermark: Arc<AtomicU64>,
) {
    let mut pending = VecDeque::new();
    loop {
        let command = match pending.pop_front() {
            Some(command) => command,
            None => match receiver.recv().await {
                Some(command) => command,
                None => break,
            },
        };
        match command {
            WriterCommand::Barrier { target, ack } => {
                let result = match failure.read().await.as_ref() {
                    Some(error) => Err(error.clone()),
                    None if durable_watermark.load(Ordering::Acquire) >= target => Ok(()),
                    None => Err(format!(
                        "durable watermark did not reach barrier target {target}"
                    )),
                };
                let _ = ack.send(result);
            }
            WriterCommand::Commit(first) => {
                let mut batch = Vec::with_capacity(MAX_COMMIT_BATCH);
                batch.push(*first);
                while batch.len() < MAX_COMMIT_BATCH {
                    match receiver.try_recv() {
                        Ok(WriterCommand::Commit(commit)) => batch.push(*commit),
                        Ok(barrier @ WriterCommand::Barrier { .. }) => {
                            pending.push_back(barrier);
                            break;
                        }
                        Err(mpsc::error::TryRecvError::Empty) => break,
                        Err(mpsc::error::TryRecvError::Disconnected) => break,
                    }
                }
                if let Some(error) = failure.read().await.as_ref().cloned() {
                    fail_batch(batch, error);
                    continue;
                }
                match persist_batch(&store, &batch, &durable_watermark).await {
                    Ok(()) => acknowledge_batch(batch),
                    Err(error) => {
                        let error = error.to_string();
                        tracing::error!(
                            error_bytes = error.len(),
                            commits = batch.len(),
                            "persistence writer faulted"
                        );
                        *failure.write().await = Some(error.clone());
                        fail_batch(batch, error);
                    }
                }
            }
        }
    }
}

async fn persist_batch(
    store: &StudioStore,
    batch: &[QueuedCommit],
    durable_watermark: &AtomicU64,
) -> anyhow::Result<()> {
    let started = std::time::Instant::now();
    let commits = batch
        .iter()
        .map(|queued| queued.commit.clone())
        .collect::<Vec<_>>();
    persist_history_batch(store.history_writer_database(), &commits).await?;
    for queued in batch {
        let outcome = super::repository::persist_state_commit(store, &queued.commit).await?;
        match outcome {
            AgentCommitOutcome::Applied => {
                durable_watermark.store(queued.request_id, Ordering::Release);
            }
            AgentCommitOutcome::RevisionConflict { actual_revision } => {
                anyhow::bail!(
                    "state projection revision conflict for agent {}: expected {:?}, actual {:?}",
                    queued.commit.agent_id,
                    queued.commit.expected_revision,
                    actual_revision
                );
            }
        }
    }
    tracing::trace!(
        commits = batch.len(),
        elapsed_ms = started.elapsed().as_millis(),
        durable_watermark = durable_watermark.load(Ordering::Acquire),
        "persisted history and state projection batch"
    );
    Ok(())
}

fn acknowledge_batch(batch: Vec<QueuedCommit>) {
    for queued in batch {
        if let Some(ack) = queued.ack {
            let _ = ack.send(Ok(AgentCommitOutcome::Applied));
        }
    }
}

fn fail_batch(batch: Vec<QueuedCommit>, error: String) {
    for queued in batch {
        if let Some(ack) = queued.ack {
            let _ = ack.send(Err(error.clone()));
        }
    }
}

fn is_deferred_checkpoint(commit: &SessionHistoryCommit) -> bool {
    matches!(commit.mutation, AgentStateMutation::ReplaceSession { .. })
        && commit.facts.runtime_events.is_empty()
        && commit.facts.trace_events.is_empty()
        && commit.facts.items.is_empty()
        && commit.facts.context.is_some()
}

fn queue_error(error: mpsc::error::TrySendError<WriterCommand>) -> PureError {
    match error {
        mpsc::error::TrySendError::Full(_) => {
            writer_error("persistence queue is full; refusing to drop durable history")
        }
        mpsc::error::TrySendError::Closed(_) => writer_error("persistence writer is closed"),
    }
}

fn writer_error(error: impl std::fmt::Display) -> PureError {
    PureError::MemoryError(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, VecDeque};

    use pl_core::{
        AgentActivityState, AgentDurableState, AgentIdentity, AgentLifecycleState, AgentRoleId,
        AgentSessionState, AgentSnapshot, DurableCommitFacts, SessionContextMutation, SessionId,
    };

    use super::*;

    #[tokio::test]
    async fn full_queue_rejects_checkpoint_without_advancing_acceptance() {
        let (sender, _receiver) = mpsc::channel(PERSISTENCE_QUEUE_CAPACITY);
        let writer = AgentPersistenceWriter {
            sender,
            acceptance: Arc::new(Mutex::new(AcceptanceState::default())),
            failure: Arc::new(RwLock::new(None)),
            durable_watermark: Arc::new(AtomicU64::new(0)),
        };

        for revision in 1..=PERSISTENCE_QUEUE_CAPACITY as u64 {
            assert_eq!(
                writer
                    .submit(deferred_checkpoint(
                        revision,
                        (revision > 1).then_some(revision - 1),
                    ))
                    .await
                    .unwrap(),
                AgentCommitOutcome::Applied
            );
        }
        let overflow = writer
            .submit(deferred_checkpoint(
                PERSISTENCE_QUEUE_CAPACITY as u64 + 1,
                Some(PERSISTENCE_QUEUE_CAPACITY as u64),
            ))
            .await;
        assert!(overflow.is_err());

        let acceptance = writer.acceptance.lock().await;
        assert_eq!(acceptance.watermark, PERSISTENCE_QUEUE_CAPACITY as u64);
        assert_eq!(
            acceptance.revisions,
            HashMap::from([(
                "agent-queue".to_string(),
                Some(PERSISTENCE_QUEUE_CAPACITY as u64),
            )])
        );
    }

    fn deferred_checkpoint(revision: u64, expected_revision: Option<u64>) -> SessionHistoryCommit {
        let session_id = SessionId::new("session-queue").unwrap();
        let state = AgentDurableState {
            snapshot: AgentSnapshot {
                identity: AgentIdentity {
                    id: pl_core::AgentId::new("agent-queue").unwrap(),
                    parent_id: None,
                    role: AgentRoleId::new("planner").unwrap(),
                    depth: 0,
                },
                lifecycle: AgentLifecycleState::Active,
                activity: AgentActivityState::Running,
                active_turn_id: None,
                pending_inputs: 0,
                progress: None,
                last_turn: None,
                revision,
                event_sequence: 0,
                updated_at: i64::try_from(revision).unwrap(),
            },
            session: AgentSessionState::empty(session_id.clone()),
            pending_inputs: VecDeque::new(),
            active_input: None,
        };
        SessionHistoryCommit {
            agent_id: state.snapshot.identity.id.clone(),
            expected_revision,
            next_state: state.clone(),
            facts: DurableCommitFacts::from_state(
                &state,
                Vec::new(),
                Vec::new(),
                None,
                Some(SessionContextMutation::Replace { items: Vec::new() }),
            ),
            mutation: AgentStateMutation::ReplaceSession { session_id },
        }
    }
}
