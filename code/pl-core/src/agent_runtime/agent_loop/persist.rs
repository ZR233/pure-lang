use pl_trace::TraceEvent;

use super::super::host::{PersistenceClass, ThreadProjectionCommit, transcript_mutation};
use super::super::state::{AgentRuntimeError, unix_timestamp};
use super::super::*;
use super::commit::{CommitPublication, PendingCommit};
use super::{AgentLoop, TRACE_BATCH_MAX_DELAY, TRACE_BATCH_MAX_EVENTS, notification_turn_id};
use crate::thread_event::{
    ObservedTurnEvent, ThreadNotificationFact, TurnObservation, project_observation,
    project_runtime_event, project_thread_facts, project_trace_events, runtime_event_thread_id,
};

pub(super) struct TransitionCommit {
    persistence_override: Option<PersistenceClass>,
    next_state: ThreadActorState,
    thread_facts: Vec<ThreadNotificationFact>,
    submission: Option<super::super::ProgressSubmissionCommit>,
}

impl TransitionCommit {
    pub(super) fn new(next_state: ThreadActorState) -> Self {
        Self {
            persistence_override: None,
            next_state,
            thread_facts: Vec::new(),
            submission: None,
        }
    }

    /// 显式标记已启动生命周期的终态收束。
    pub(super) fn settlement(mut self) -> Self {
        self.persistence_override = Some(PersistenceClass::Settlement);
        self
    }

    pub(super) fn with_thread_facts(mut self, thread_facts: Vec<ThreadNotificationFact>) -> Self {
        self.thread_facts = thread_facts;
        self
    }

    pub(super) fn with_submission(
        mut self,
        submission: super::super::ProgressSubmissionCommit,
    ) -> Self {
        self.submission = Some(submission);
        self
    }
}

impl<H> AgentLoop<H>
where
    H: AgentRuntimeHost,
{
    pub(super) async fn persist_turn_observation(
        &mut self,
        observation: TurnObservation,
    ) -> AgentRuntimeResult<()> {
        let Some(active) = &self.active else {
            return Ok(());
        };
        self.persist_observation(ObservedTurnEvent {
            turn_id: active.turn_id.to_string(),
            thread_id: active.thread_id.to_string(),
            observation,
        })
        .await
    }

    pub(super) async fn persist_observation(
        &mut self,
        observed: ObservedTurnEvent,
    ) -> AgentRuntimeResult<()> {
        let Some(active) = &self.active else {
            return Ok(());
        };
        if active.turn_id.as_str() != observed.turn_id
            || active.thread_id.as_str() != observed.thread_id
            || active.is_cancelling()
        {
            return Ok(());
        }
        let thread_id = active.thread_id.clone();
        let turn_id = active.turn_id.clone();
        let persistence = observation_persistence(&observed.observation);
        let plan_update = match &observed.observation {
            TurnObservation::InteractionChanged(interaction)
                if interaction.status() == pl_protocol::InteractionStatus::Pending =>
            {
                crate::session::plan::state_for_pending_interaction(
                    self.state.session.session.plan(),
                    interaction,
                )
                .map_err(AgentRuntimeError::InvalidInput)?
            }
            TurnObservation::RuntimeDelta(_)
            | TurnObservation::TodoList(_)
            | TurnObservation::InteractionChanged(_)
            | TurnObservation::ContextCompacted { .. }
            | TurnObservation::Diagnostic => None,
        };
        let expected_revision = self.state.snapshot.revision;
        let current = self
            .runtime
            .thread_events
            .snapshot(thread_id.as_str())
            .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?;
        let projected = project_observation(
            thread_id.as_str(),
            turn_id.as_str(),
            current.revision,
            &current,
            observed.observation,
        );
        // bus 是唯一 ordinal 分配者：规范化通知同时供给快照、facts 与广播。
        let projected_thread = self
            .runtime
            .thread_events
            .project(thread_id.as_str(), &projected.notifications)
            .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?;
        let projection = ThreadProjectionCommit {
            snapshot: projected_thread.snapshot,
            notifications: projected_thread.notifications.clone(),
        };
        let mut next = self.state.clone();
        if let Some(plan) = plan_update {
            next.session.session.replace_plan(Some(plan));
        }
        next.snapshot.revision = expected_revision.saturating_add(1);
        next.snapshot.updated_at = unix_timestamp();
        next.session.thread_revision = projected.through_revision;
        let facts =
            DurableCommitFacts::from_state(&next, Vec::new(), Vec::new(), Some(projection), None);
        self.commit_and_publish(
            PendingCommit::new(
                next,
                facts,
                ThreadMutation::AppendThreadNotifications {
                    thread_id: thread_id.clone(),
                },
            )
            .persistence(persistence)
            .publish(
                CommitPublication::new(Some(thread_id), Some(turn_id))
                    .store_directory_snapshot()
                    .with_thread_notifications(projected_thread.notifications),
            ),
        )
        .await
    }

    pub(super) async fn persist_trace_batch(
        &mut self,
        mut trace_events: Vec<TraceEvent>,
    ) -> AgentRuntimeResult<()> {
        self.pending_trace_events.append(&mut trace_events);
        while let Ok(trace) = self.channels.trace_receiver.try_recv() {
            self.pending_trace_events.push(trace);
        }
        if !self.pending_trace_events.is_empty()
            && self.pending_trace_events.len() < TRACE_BATCH_MAX_EVENTS
        {
            let deadline = tokio::time::Instant::now() + TRACE_BATCH_MAX_DELAY;
            while self.pending_trace_events.len() < TRACE_BATCH_MAX_EVENTS {
                match tokio::time::timeout_at(deadline, self.channels.trace_receiver.recv()).await {
                    Ok(Some(trace)) => self.pending_trace_events.push(trace),
                    Ok(None) | Err(_) => break,
                }
            }
        }
        let Some(active) = &self.active else {
            return Ok(());
        };
        let thread_id = active.thread_id.clone();
        let turn_id = active.turn_id.clone();
        if self
            .pending_trace_events
            .iter()
            .any(|trace| trace.session_id != thread_id.as_str())
        {
            return Err(AgentRuntimeError::ThreadEvents(format!(
                "trace session mismatch for agent {}",
                self.state.snapshot.identity.id
            )));
        }
        self.pending_trace_events
            .sort_by_key(|trace| trace.sequence);
        let current_sequence = self.state.session.trace_sequence;
        self.pending_trace_events
            .retain(|trace| trace.sequence >= current_sequence);
        if self.pending_trace_events.is_empty() {
            return Ok(());
        }
        let trace_events = self.pending_trace_events.clone();
        let next_trace_sequence = trace_events
            .last()
            .map(|trace| trace.sequence.saturating_add(1))
            .unwrap_or(current_sequence);
        let expected_revision = self.state.snapshot.revision;
        let current_thread = self
            .runtime
            .thread_events
            .snapshot(thread_id.as_str())
            .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?;
        let projected = project_trace_events(thread_id.as_str(), &current_thread, &trace_events);
        let projected_thread = if projected.notifications.is_empty() {
            None
        } else {
            Some(
                self.runtime
                    .thread_events
                    .project(thread_id.as_str(), &projected.notifications)
                    .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?,
            )
        };
        let session_projection =
            projected_thread
                .as_ref()
                .map(|projected_thread| ThreadProjectionCommit {
                    snapshot: projected_thread.snapshot.clone(),
                    notifications: projected_thread.notifications.clone(),
                });
        let mut next = self.state.clone();
        next.snapshot.revision = expected_revision.saturating_add(1);
        next.snapshot.updated_at = unix_timestamp();
        next.session.trace_sequence = next_trace_sequence;
        next.session.thread_revision = projected.through_revision;
        let committed_trace_events = trace_events.clone();
        let committed_thread_events = projected_thread
            .as_ref()
            .map_or_else(Vec::new, |projected_thread| {
                projected_thread.notifications.clone()
            });
        let facts = DurableCommitFacts::from_state(
            &next,
            Vec::new(),
            trace_events,
            session_projection,
            None,
        );
        let result = self
            .commit_and_publish(
                PendingCommit::new(next, facts, ThreadMutation::AppendTrace).publish(
                    CommitPublication::new(Some(thread_id), Some(turn_id))
                        .store_directory_snapshot()
                        .with_trace_events(committed_trace_events)
                        .with_thread_notifications(committed_thread_events),
                ),
            )
            .await;
        if result.is_ok() {
            self.pending_trace_events
                .retain(|trace| trace.sequence >= next_trace_sequence);
        }
        result
    }

    pub(super) async fn commit_transition<F>(
        &mut self,
        transition: TransitionCommit,
        event_kind: F,
    ) -> AgentRuntimeResult<()>
    where
        F: FnOnce(AgentSnapshot) -> AgentRuntimeEventKind,
    {
        let TransitionCommit {
            persistence_override,
            mut next_state,
            thread_facts,
            submission,
        } = transition;
        let expected_revision = self.state.snapshot.revision;
        let context = transcript_mutation(
            self.state.session.session.items(),
            next_state.session.session.items(),
        );
        next_state.snapshot.revision = expected_revision.saturating_add(1);
        next_state.snapshot.event_sequence = self.state.snapshot.event_sequence.saturating_add(1);
        next_state.snapshot.updated_at = unix_timestamp();
        let event = AgentRuntimeEvent {
            agent_id: next_state.snapshot.identity.id.clone(),
            sequence: next_state.snapshot.event_sequence,
            created_at: next_state.snapshot.updated_at,
            kind: event_kind(next_state.snapshot.clone()),
        };
        let thread_id = runtime_event_thread_id(&event).map(str::to_string);
        let current_thread = match thread_id.as_deref() {
            Some(thread_id) => Some(
                self.runtime
                    .thread_events
                    .snapshot(thread_id)
                    .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?,
            ),
            None if thread_facts.is_empty() => None,
            None => {
                return Err(AgentRuntimeError::InvalidInput(
                    "thread facts require a thread-scoped runtime event".to_string(),
                ));
            }
        };
        let empty_thread;
        let current_for_projection = match current_thread.as_ref() {
            Some(current) => current,
            None => {
                empty_thread =
                    pl_protocol::ThreadSnapshot::empty(self.state.snapshot.identity.id.to_string());
                &empty_thread
            }
        };
        let mut projected = project_runtime_event(&event, current_for_projection);
        if !thread_facts.is_empty() {
            let Some(current) = current_thread.as_ref() else {
                return Err(AgentRuntimeError::InvalidInput(
                    "thread facts require a current thread snapshot".to_string(),
                ));
            };
            let after_runtime = if projected.notifications.is_empty() {
                current.clone()
            } else {
                self.runtime
                    .thread_events
                    .project(current.thread.id.as_str(), &projected.notifications)
                    .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?
                    .snapshot
            };
            let extra =
                project_thread_facts(current.thread.id.as_str(), &after_runtime, thread_facts);
            projected.through_revision = extra.through_revision;
            projected.notifications.extend(extra.notifications);
        }
        let projected_thread = match thread_id.as_deref() {
            Some(thread_id) if !projected.notifications.is_empty() => Some(
                self.runtime
                    .thread_events
                    .project(thread_id, &projected.notifications)
                    .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?,
            ),
            Some(_) | None => None,
        };
        let thread_projection =
            projected_thread
                .as_ref()
                .map(|projected_thread| ThreadProjectionCommit {
                    snapshot: projected_thread.snapshot.clone(),
                    notifications: projected_thread.notifications.clone(),
                });
        if let Some(thread_id) = thread_id.as_ref() {
            let thread_id = ThreadId::new(thread_id.clone())
                .map_err(|error| AgentRuntimeError::Repository(error.to_string()))?;
            if next_state.snapshot.identity.id != thread_id {
                return Err(AgentRuntimeError::ThreadMismatch {
                    agent_id: next_state.snapshot.identity.id.clone(),
                    expected: next_state.snapshot.identity.id.clone(),
                    actual: thread_id,
                });
            }
            next_state.session.thread_revision = projected.through_revision;
        }
        let committed_thread_events = projected_thread
            .as_ref()
            .map_or_else(Vec::new, |projected_thread| {
                projected_thread.notifications.clone()
            });
        let mut facts = DurableCommitFacts::from_state(
            &next_state,
            vec![event.clone()],
            Vec::new(),
            thread_projection,
            context,
        );
        facts.submission = submission;
        let published_thread_id = thread_id.and_then(|value| ThreadId::new(value).ok());
        let published_turn_id = projected
            .notifications
            .first()
            .and_then(notification_turn_id)
            .and_then(|value| TurnId::new(value.to_string()).ok());
        let persistence =
            persistence_override.unwrap_or_else(|| PersistenceClass::for_event(&event.kind));
        self.commit_and_publish(
            PendingCommit::new(next_state, facts, ThreadMutation::SnapshotAndQueue)
                .persistence(persistence)
                .publish(
                    CommitPublication::new(published_thread_id, published_turn_id)
                        .with_runtime_event(event)
                        .with_thread_notifications(committed_thread_events),
                ),
        )
        .await
    }
}

/// Interaction 需要保留终态容量；其余 observation 可以作为流式增量调度。
fn observation_persistence(observation: &TurnObservation) -> PersistenceClass {
    match observation {
        TurnObservation::InteractionChanged { .. } => PersistenceClass::Settlement,
        TurnObservation::RuntimeDelta(_)
        | TurnObservation::TodoList(_)
        | TurnObservation::ContextCompacted { .. }
        | TurnObservation::Diagnostic => PersistenceClass::Coalescible,
    }
}
