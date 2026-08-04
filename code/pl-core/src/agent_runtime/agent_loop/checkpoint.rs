use std::collections::BTreeSet;

use super::super::host::{AgentCommitObserver, AgentStateRepository, SessionProjectionCommit};
use super::super::state::{AgentRuntimeError, unix_timestamp};
use super::super::{
    AgentActivityState, AgentCommitOutcome, AgentCommittedEvent, AgentLifecycleState,
    AgentProgressCheckpoint, AgentProgressStage, AgentRuntimeEventKind, AgentRuntimeHost,
    AgentRuntimeResult, AgentStateMutation, AgentTurnCheckpoint, DurableCommitFacts,
    MailboxDeliveryState, SessionContextMutation, SessionHistoryCommit, SessionId, TurnId,
};
use super::AgentLoop;

impl<H> AgentLoop<H>
where
    H: AgentRuntimeHost,
{
    pub(super) async fn set_activity(
        &mut self,
        turn_id: TurnId,
        activity: AgentActivityState,
    ) -> AgentRuntimeResult<()> {
        let Some(active) = &self.active else {
            return Ok(());
        };
        if active.turn_id != turn_id
            || self.state.snapshot.lifecycle != AgentLifecycleState::Active
            || self.state.snapshot.activity == activity
        {
            return Ok(());
        }
        let mut next = self.state.clone();
        next.snapshot.activity = activity;
        self.commit_transition(next, Vec::new(), |snapshot| {
            AgentRuntimeEventKind::StateChanged { snapshot }
        })
        .await
    }

    pub(super) async fn checkpoint(
        &mut self,
        checkpoint: AgentTurnCheckpoint,
    ) -> AgentRuntimeResult<()> {
        self.flush_pending_traces().await?;
        let Some(active) = &self.active else {
            return Ok(());
        };
        if active.turn_id != checkpoint.turn_id
            || active.session_id != checkpoint.session_id
            || active.cancelling
            || active.cancellation.is_cancelled()
            || checkpoint.sequence <= active.checkpoint_sequence
        {
            return Ok(());
        }
        let expected_revision = self.state.snapshot.revision;
        let mut next = self.state.clone();
        next.snapshot.revision = expected_revision.saturating_add(1);
        next.snapshot.updated_at = unix_timestamp();
        if let Some(active_input) = next.active_input.as_mut()
            && active_input.turn_id == checkpoint.turn_id
        {
            active_input.consume(checkpoint.sequence);
        }
        if !checkpoint.consumed_mail_ids.is_empty() {
            let consumed = checkpoint
                .consumed_mail_ids
                .iter()
                .map(String::as_str)
                .collect::<BTreeSet<_>>();
            next.pending_inputs.retain(|input| {
                !consumed.contains(input.mail_id.as_str())
                    || !matches!(
                        &input.delivery_state,
                        MailboxDeliveryState::Claimed { turn_id, .. }
                            if turn_id == &checkpoint.turn_id
                    )
            });
            next.refresh_mailbox_snapshot();
        }
        if next.session.id != checkpoint.session_id {
            return Err(AgentRuntimeError::SessionMismatch {
                agent_id: next.snapshot.identity.id.clone(),
                expected: next.session.id.clone(),
                actual: checkpoint.session_id,
            });
        }
        next.session.session = checkpoint.session;
        let context = SessionContextMutation::Replace {
            items: next.session.session.items().to_vec(),
        };
        let result = self
            .host
            .repository()
            .commit(SessionHistoryCommit {
                agent_id: next.snapshot.identity.id.clone(),
                expected_revision: Some(expected_revision),
                next_state: next.clone(),
                facts: DurableCommitFacts::from_state(
                    &next,
                    Vec::new(),
                    Vec::new(),
                    None,
                    Some(context),
                ),
                mutation: AgentStateMutation::ReplaceSession {
                    session_id: checkpoint.session_id.clone(),
                },
            })
            .await
            .map_err(|error| AgentRuntimeError::Repository(error.to_string()))?;
        match result {
            AgentCommitOutcome::Applied => {
                tracing::trace!(
                    agent_id = %next.snapshot.identity.id,
                    turn_id = %checkpoint.turn_id,
                    sequence = checkpoint.sequence,
                    reason = ?checkpoint.reason,
                    "agent turn checkpoint committed"
                );
                self.state = next;
                if let Some(active) = &mut self.active {
                    active.checkpoint_sequence = checkpoint.sequence;
                }
                Ok(())
            }
            AgentCommitOutcome::RevisionConflict { actual_revision } => {
                Err(AgentRuntimeError::RevisionConflict {
                    expected: Some(expected_revision),
                    actual: actual_revision,
                })
            }
        }
    }

    pub(super) async fn record_session_facts(
        &mut self,
        session_id: SessionId,
        mut facts: Vec<crate::SessionEventFact>,
    ) -> AgentRuntimeResult<()> {
        if facts.is_empty() {
            return Ok(());
        }
        if self.state.session.id != session_id {
            return Err(AgentRuntimeError::SessionMismatch {
                agent_id: self.state.snapshot.identity.id.clone(),
                expected: self.state.session.id.clone(),
                actual: session_id,
            });
        }
        let owner_agent_id = self.state.snapshot.identity.id.to_string();
        for fact in &mut facts {
            match fact.source_agent_id.as_deref() {
                Some(source_agent_id) if source_agent_id != owner_agent_id => {
                    return Err(AgentRuntimeError::Repository(format!(
                        "session {session_id} belongs to agent {owner_agent_id}, \
                         but fact source is {source_agent_id}"
                    )));
                }
                Some(_) => {}
                None => fact.source_agent_id = Some(owner_agent_id.clone()),
            }
        }
        let current = self
            .runtime
            .session_events
            .snapshot(session_id.as_str())
            .map_err(|error| AgentRuntimeError::SessionEvents(error.to_string()))?;
        let projected = crate::session_event::project_session_facts(
            session_id.as_str(),
            current.through_sequence,
            facts,
        );
        let durable_events = projected.durable_events();
        if durable_events.is_empty() {
            self.runtime
                .session_events
                .publish_batch(projected.events)
                .map_err(|error| AgentRuntimeError::SessionEvents(error.to_string()))?;
            return Ok(());
        }

        let expected_revision = self.state.snapshot.revision;
        let projection = SessionProjectionCommit {
            snapshot: self
                .runtime
                .session_events
                .project_durable(session_id.as_str(), &durable_events)
                .map_err(|error| AgentRuntimeError::SessionEvents(error.to_string()))?,
            durable_events,
        };
        let mut next = self.state.clone();
        next.snapshot.revision = expected_revision.saturating_add(1);
        next.snapshot.updated_at = unix_timestamp();
        next.session.session_event_sequence = projected.through_sequence;
        let outcome = self
            .host
            .repository()
            .commit(SessionHistoryCommit {
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
                mutation: AgentStateMutation::AppendSessionEvents {
                    session_id: session_id.clone(),
                },
            })
            .await
            .map_err(|error| AgentRuntimeError::Repository(error.to_string()))?;
        match outcome {
            AgentCommitOutcome::Applied => {
                let agent_id = next.snapshot.identity.id.clone();
                self.state = next;
                self.runtime
                    .session_events
                    .publish_batch(projected.events.clone())
                    .map_err(|error| AgentRuntimeError::SessionEvents(error.to_string()))?;
                self.host
                    .observer()
                    .publish(AgentCommittedEvent {
                        agent_id,
                        session_id: Some(session_id),
                        turn_id: projected
                            .events
                            .first()
                            .and_then(|event| event.turn_id.clone())
                            .and_then(|value| TurnId::new(value).ok()),
                        runtime_events: Vec::new(),
                        trace_events: Vec::new(),
                        session_events: projected.events,
                    })
                    .await;
                Ok(())
            }
            AgentCommitOutcome::RevisionConflict { actual_revision } => {
                Err(AgentRuntimeError::RevisionConflict {
                    expected: Some(expected_revision),
                    actual: actual_revision,
                })
            }
        }
    }

    pub(super) async fn report_progress(
        &mut self,
        stage: AgentProgressStage,
        summary: String,
        next_step: String,
    ) -> AgentRuntimeResult<AgentProgressCheckpoint> {
        let summary = bounded_required_text("summary", summary, 1_200)?;
        let next_step = bounded_required_text("nextStep", next_step, 500)?;
        if let Some(current) = &self.state.snapshot.progress
            && current.stage == stage
            && current.summary == summary
            && current.next_step == next_step
        {
            return Ok(current.clone());
        }
        let revision = self
            .state
            .snapshot
            .progress
            .as_ref()
            .map_or(1, |progress| progress.revision.saturating_add(1));
        let checkpoint = AgentProgressCheckpoint {
            stage,
            summary,
            next_step,
            revision,
            updated_at: unix_timestamp(),
        };
        let mut next = self.state.clone();
        next.snapshot.progress = Some(checkpoint.clone());
        self.commit_transition(next, Vec::new(), |snapshot| {
            AgentRuntimeEventKind::StateChanged { snapshot }
        })
        .await?;
        Ok(checkpoint)
    }
}

fn bounded_required_text(
    field: &str,
    value: String,
    max_chars: usize,
) -> AgentRuntimeResult<String> {
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err(AgentRuntimeError::InvalidInput(format!(
            "{field} must not be empty"
        )));
    }
    if value.chars().count() > max_chars {
        return Err(AgentRuntimeError::InvalidInput(format!(
            "{field} exceeds {max_chars} characters"
        )));
    }
    Ok(value)
}
