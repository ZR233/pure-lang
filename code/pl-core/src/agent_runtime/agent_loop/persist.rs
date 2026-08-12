use pl_trace::TraceEvent;

use super::super::host::{
    AgentCommitObserver, ThreadProjectionCommit, ThreadRepository, transcript_mutation,
};
use super::super::state::{ActiveTurnActivity, AgentRuntimeError, derive_activity, unix_timestamp};
use super::super::*;
use super::{AgentLoop, notification_turn_id};
use crate::thread_event::{
    ObservedTurnEvent, ThreadNotificationFact, TurnObservation, project_observation,
    project_runtime_event, project_thread_facts, project_trace_events, runtime_event_thread_id,
};

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
        let outcome = self
            .host
            .repository()
            .commit(ThreadCommit {
                agent_id: next.snapshot.identity.id.clone(),
                expected_revision: Some(expected_revision),
                next_state: next.clone(),
                facts: DurableCommitFacts::from_state(
                    &next,
                    Vec::new(),
                    Vec::new(),
                    Some(projection),
                    None,
                ),
                mutation: ThreadMutation::AppendThreadNotifications {
                    thread_id: thread_id.clone(),
                },
            })
            .await
            .map_err(|error| AgentRuntimeError::Repository(error.to_string()))?;
        match outcome {
            ThreadCommitOutcome::Applied => {
                self.state = next;
                self.runtime
                    .directory
                    .store_snapshot(self.state.snapshot.clone());
                self.runtime
                    .thread_events
                    .publish_batch(projected.notifications.clone())
                    .await
                    .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?;
                self.host
                    .observer()
                    .publish(AgentCommittedEvent {
                        agent_id: self.state.snapshot.identity.id.clone(),
                        thread_id: Some(thread_id),
                        turn_id: Some(turn_id),
                        runtime_events: Vec::new(),
                        trace_events: Vec::new(),
                        thread_notifications: projected.notifications,
                    })
                    .await;
                Ok(())
            }
            ThreadCommitOutcome::RevisionConflict { actual_revision } => {
                Err(AgentRuntimeError::RevisionConflict {
                    expected: Some(expected_revision),
                    actual: actual_revision,
                })
            }
        }
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
        let result = self
            .host
            .repository()
            .commit(ThreadCommit {
                agent_id: next.snapshot.identity.id.clone(),
                expected_revision: Some(expected_revision),
                next_state: next.clone(),
                facts: DurableCommitFacts::from_state(
                    &next,
                    Vec::new(),
                    trace_events,
                    session_projection,
                    None,
                ),
                mutation: ThreadMutation::AppendTrace,
            })
            .await
            .map_err(|error| AgentRuntimeError::Repository(error.to_string()))?;
        match result {
            ThreadCommitOutcome::Applied => {
                let agent_id = next.snapshot.identity.id.clone();
                self.state = next;
                self.runtime
                    .directory
                    .store_snapshot(self.state.snapshot.clone());
                self.runtime
                    .thread_events
                    .publish_batch(committed_thread_events.clone())
                    .await
                    .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?;
                self.host
                    .observer()
                    .publish(AgentCommittedEvent {
                        agent_id,
                        thread_id: Some(thread_id),
                        turn_id: Some(turn_id),
                        runtime_events: Vec::new(),
                        trace_events: committed_trace_events,
                        thread_notifications: committed_thread_events,
                    })
                    .await;
                Ok(())
            }
            ThreadCommitOutcome::RevisionConflict { actual_revision } => {
                Err(AgentRuntimeError::RevisionConflict {
                    expected: Some(expected_revision),
                    actual: actual_revision,
                })
            }
        }
    }

    pub(super) async fn commit_transition<F>(
        &mut self,
        next: ThreadActorState,
        trace_events: Vec<TraceEvent>,
        event_kind: F,
    ) -> AgentRuntimeResult<()>
    where
        F: FnOnce(AgentSnapshot) -> AgentRuntimeEventKind,
    {
        self.commit_transition_with_thread_facts(next, trace_events, Vec::new(), None, event_kind)
            .await
    }

    /// 与 [`commit_transition`] 相同，但同时把一次阶段提交原子追加到
    /// `thread_submissions`（同一事务）。
    pub(super) async fn commit_progress_transition<F>(
        &mut self,
        next: ThreadActorState,
        submission: super::super::ProgressSubmissionCommit,
        event_kind: F,
    ) -> AgentRuntimeResult<()>
    where
        F: FnOnce(AgentSnapshot) -> AgentRuntimeEventKind,
    {
        self.commit_transition_with_thread_facts(
            next,
            Vec::new(),
            Vec::new(),
            Some(submission),
            event_kind,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn commit_transition_with_thread_facts<F>(
        &mut self,
        mut next: ThreadActorState,
        trace_events: Vec<TraceEvent>,
        thread_facts: Vec<ThreadNotificationFact>,
        submission: Option<super::super::ProgressSubmissionCommit>,
        event_kind: F,
    ) -> AgentRuntimeResult<()>
    where
        F: FnOnce(AgentSnapshot) -> AgentRuntimeEventKind,
    {
        let expected_revision = self.state.snapshot.revision;
        let active = next.snapshot.active_turn_id.as_ref().map(|_| {
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
        next.snapshot.activity =
            derive_activity(next.snapshot.lifecycle, active, next.has_triggering_input());
        let context = transcript_mutation(
            self.state.session.session.items(),
            next.session.session.items(),
        );
        next.snapshot.revision = expected_revision.saturating_add(1);
        next.snapshot.event_sequence = self.state.snapshot.event_sequence.saturating_add(1);
        next.snapshot.updated_at = unix_timestamp();
        let event = AgentRuntimeEvent {
            agent_id: next.snapshot.identity.id.clone(),
            sequence: next.snapshot.event_sequence,
            created_at: next.snapshot.updated_at,
            kind: event_kind(next.snapshot.clone()),
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
            if next.snapshot.identity.id != thread_id {
                return Err(AgentRuntimeError::ThreadMismatch {
                    agent_id: next.snapshot.identity.id.clone(),
                    expected: next.snapshot.identity.id.clone(),
                    actual: thread_id,
                });
            }
            next.session.thread_revision = projected.through_revision;
        }
        let committed_trace_events = trace_events.clone();
        let committed_thread_events = projected.notifications.clone();
        let mut facts = DurableCommitFacts::from_state(
            &next,
            vec![event.clone()],
            trace_events,
            thread_projection,
            context,
        );
        facts.submission = submission;
        let result = self
            .host
            .repository()
            .commit(ThreadCommit {
                agent_id: next.snapshot.identity.id.clone(),
                expected_revision: Some(expected_revision),
                next_state: next.clone(),
                facts,
                mutation: ThreadMutation::SnapshotAndQueue,
            })
            .await
            .map_err(|error| AgentRuntimeError::Repository(error.to_string()))?;
        match result {
            ThreadCommitOutcome::Applied => {
                self.state = next;
                self.runtime.directory.publish_runtime_event(&event);
                self.runtime
                    .thread_events
                    .publish_batch(committed_thread_events.clone())
                    .await
                    .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?;
                self.host
                    .observer()
                    .publish(AgentCommittedEvent {
                        agent_id: event.agent_id.clone(),
                        thread_id: thread_id.and_then(|value| ThreadId::new(value).ok()),
                        turn_id: projected
                            .notifications
                            .first()
                            .and_then(notification_turn_id)
                            .and_then(|value| TurnId::new(value.to_string()).ok()),
                        runtime_events: vec![event],
                        trace_events: committed_trace_events,
                        thread_notifications: committed_thread_events,
                    })
                    .await;
                Ok(())
            }
            ThreadCommitOutcome::RevisionConflict { actual_revision } => {
                Err(AgentRuntimeError::RevisionConflict {
                    expected: Some(expected_revision),
                    actual: actual_revision,
                })
            }
        }
    }
}
