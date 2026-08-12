use pl_trace::TraceEvent;

use super::super::host::{ThreadProjectionCommit, transcript_mutation};
use super::super::state::{ActiveTurnActivity, AgentRuntimeError, derive_activity, unix_timestamp};
use super::super::*;
use super::commit::{CommitPublication, PendingCommit};
use super::{AgentLoop, notification_turn_id};
use crate::thread_event::{
    ObservedTurnEvent, ThreadNotificationFact, TurnObservation, project_observation,
    project_runtime_event, project_thread_facts, project_trace_events, runtime_event_thread_id,
};

pub(super) struct TransitionCommit {
    next_state: ThreadActorState,
    thread_facts: Vec<ThreadNotificationFact>,
    submission: Option<super::super::ProgressSubmissionCommit>,
}

impl TransitionCommit {
    pub(super) fn new(next_state: ThreadActorState) -> Self {
        Self {
            next_state,
            thread_facts: Vec::new(),
            submission: None,
        }
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
            || active.cancelling
        {
            return Ok(());
        }
        let thread_id = active.thread_id.clone();
        let turn_id = active.turn_id.clone();
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
        let projection = ThreadProjectionCommit {
            snapshot: self
                .runtime
                .thread_events
                .project(thread_id.as_str(), &projected.notifications)
                .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?,
            notifications: projected.notifications.clone(),
        };
        let mut next = self.state.clone();
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
            .publish(
                CommitPublication::new(Some(thread_id), Some(turn_id))
                    .store_directory_snapshot()
                    .with_thread_notifications(projected.notifications),
            ),
        )
        .await
    }

    pub(super) async fn persist_trace_batch(
        &mut self,
        mut trace_events: Vec<TraceEvent>,
    ) -> AgentRuntimeResult<()> {
        while let Ok(trace) = self.channels.trace_receiver.try_recv() {
            trace_events.push(trace);
        }
        let Some(active) = &self.active else {
            return Ok(());
        };
        let thread_id = active.thread_id.clone();
        let turn_id = active.turn_id.clone();
        if trace_events
            .iter()
            .any(|trace| trace.session_id != thread_id.as_str())
        {
            return Err(AgentRuntimeError::Repository(format!(
                "trace session mismatch for agent {}",
                self.state.snapshot.identity.id
            )));
        }
        trace_events.sort_by_key(|trace| trace.sequence);
        let current_sequence = self.state.session.trace_sequence;
        trace_events.retain(|trace| trace.sequence >= current_sequence);
        if trace_events.is_empty() {
            return Ok(());
        }
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
        let session_projection = if projected.notifications.is_empty() {
            None
        } else {
            Some(ThreadProjectionCommit {
                snapshot: self
                    .runtime
                    .thread_events
                    .project(thread_id.as_str(), &projected.notifications)
                    .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?,
                notifications: projected.notifications.clone(),
            })
        };
        let mut next = self.state.clone();
        next.snapshot.revision = expected_revision.saturating_add(1);
        next.snapshot.updated_at = unix_timestamp();
        next.session.trace_sequence = next_trace_sequence;
        next.session.thread_revision = projected.through_revision;
        let committed_trace_events = trace_events.clone();
        let committed_thread_events = projected.notifications.clone();
        let facts = DurableCommitFacts::from_state(
            &next,
            Vec::new(),
            trace_events,
            session_projection,
            None,
        );
        self.commit_and_publish(
            PendingCommit::new(next, facts, ThreadMutation::AppendTrace).publish(
                CommitPublication::new(Some(thread_id), Some(turn_id))
                    .store_directory_snapshot()
                    .with_trace_events(committed_trace_events)
                    .with_thread_notifications(committed_thread_events),
            ),
        )
        .await
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
            mut next_state,
            thread_facts,
            submission,
        } = transition;
        let expected_revision = self.state.snapshot.revision;
        let active = next_state.snapshot.active_turn_id.as_ref().map(|_| {
            self.active.as_ref().map_or(
                ActiveTurnActivity {
                    kind: ActiveKind::Running,
                    cancelling: false,
                },
                |active| ActiveTurnActivity {
                    kind: active.kind,
                    cancelling: active.cancelling,
                },
            )
        });
        next_state.snapshot.activity = derive_activity(
            next_state.snapshot.lifecycle,
            active,
            next_state.has_triggering_input(),
        );
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
        let current_thread_revision = current_thread
            .as_ref()
            .map_or(0, |snapshot| snapshot.revision);
        let mut projected = project_runtime_event(&event, current_thread_revision);
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
            };
            let extra =
                project_thread_facts(current.thread.id.as_str(), &after_runtime, thread_facts);
            projected.through_revision = extra.through_revision;
            projected.notifications.extend(extra.notifications);
        }
        let thread_projection = match thread_id.as_deref() {
            Some(thread_id) if !projected.notifications.is_empty() => {
                Some(ThreadProjectionCommit {
                    snapshot: self
                        .runtime
                        .thread_events
                        .project(thread_id, &projected.notifications)
                        .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?,
                    notifications: projected.notifications.clone(),
                })
            }
            Some(_) | None => None,
        };
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
        let committed_thread_events = projected.notifications.clone();
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
        self.commit_and_publish(
            PendingCommit::new(next_state, facts, ThreadMutation::SnapshotAndQueue).publish(
                CommitPublication::new(published_thread_id, published_turn_id)
                    .with_runtime_event(event)
                    .with_thread_notifications(committed_thread_events),
            ),
        )
        .await
    }
}
