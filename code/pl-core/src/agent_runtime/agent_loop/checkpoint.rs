use std::collections::BTreeSet;

use super::super::host::{
    AgentCommitObserver, ThreadProjectionCommit, ThreadRepository, transcript_mutation,
};
use super::super::state::{AgentRuntimeError, unix_timestamp};
use super::super::{
    AgentActivityState, AgentCommittedEvent, AgentInferenceCommit, AgentLifecycleState,
    AgentProgressCheckpoint, AgentProgressStage, AgentRuntimeEventKind, AgentRuntimeHost,
    AgentRuntimeResult, AgentTurnCheckpoint, DurableCommitFacts, MailboxDeliveryState,
    ThreadCommit, ThreadCommitOutcome, ThreadId, ThreadMutation, TurnId,
};
use super::AgentLoop;
use crate::{
    ThreadNotificationFact,
    thread_event::{TurnObservation, project_observation, project_thread_facts},
};

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
            || active.thread_id != checkpoint.thread_id
            || active.cancelling
            || active.cancellation.is_cancelled()
        {
            return Ok(());
        }
        if let Some(inference) = checkpoint.inference.as_ref()
            && let Some(existing) = find_inference(&self.state, &inference.billing.inference_id)
        {
            return if existing == &inference.billing {
                Ok(())
            } else {
                Err(AgentRuntimeError::InvalidInput(format!(
                    "inference {} conflicts with the active Thread billing record",
                    inference.billing.inference_id
                )))
            };
        }
        if checkpoint.sequence <= active.checkpoint_sequence {
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
        if next.snapshot.identity.id != checkpoint.thread_id {
            return Err(AgentRuntimeError::ThreadMismatch {
                agent_id: next.snapshot.identity.id.clone(),
                expected: next.snapshot.identity.id.clone(),
                actual: checkpoint.thread_id,
            });
        }
        let context = transcript_mutation(
            self.state.session.session.items(),
            checkpoint.session.items(),
        );
        next.session.session = checkpoint.session;
        let projection = if let Some(inference) = checkpoint.inference.as_ref() {
            append_inference(&mut next, &checkpoint.turn_id, inference)?;
            let current = self
                .runtime
                .thread_events
                .snapshot(checkpoint.thread_id.as_str())
                .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?;
            let projected = project_observation(
                checkpoint.thread_id.as_str(),
                checkpoint.turn_id.as_str(),
                current.revision,
                &current,
                TurnObservation::RuntimeDelta(inference.runtime_delta.clone()),
            );
            next.session.thread_revision = projected.through_revision;
            Some(ThreadProjectionCommit {
                snapshot: self
                    .runtime
                    .thread_events
                    .project(checkpoint.thread_id.as_str(), &projected.notifications)
                    .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?,
                notifications: projected.notifications,
            })
        } else {
            None
        };
        let mut facts = DurableCommitFacts::from_state(
            &next,
            Vec::new(),
            Vec::new(),
            projection.clone(),
            context,
        );
        facts.inference = checkpoint.inference.clone();
        let result = self
            .host
            .repository()
            .commit(ThreadCommit {
                agent_id: next.snapshot.identity.id.clone(),
                expected_revision: Some(expected_revision),
                next_state: next.clone(),
                facts,
                mutation: ThreadMutation::ReplaceThread {
                    thread_id: checkpoint.thread_id.clone(),
                },
            })
            .await
            .map_err(|error| AgentRuntimeError::Repository(error.to_string()))?;
        match result {
            ThreadCommitOutcome::Applied => {
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
                if let Some(projection) = projection {
                    self.runtime
                        .thread_events
                        .publish_batch(projection.notifications.clone())
                        .await
                        .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?;
                    self.host
                        .observer()
                        .publish(AgentCommittedEvent {
                            agent_id: self.state.snapshot.identity.id.clone(),
                            thread_id: Some(checkpoint.thread_id),
                            turn_id: Some(checkpoint.turn_id),
                            runtime_events: Vec::new(),
                            trace_events: Vec::new(),
                            thread_notifications: projection.notifications,
                        })
                        .await;
                }
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

    pub(super) async fn record_thread_facts(
        &mut self,
        thread_id: ThreadId,
        facts: Vec<ThreadNotificationFact>,
    ) -> AgentRuntimeResult<()> {
        if facts.is_empty() {
            return Ok(());
        }
        if self.state.snapshot.identity.id != thread_id {
            return Err(AgentRuntimeError::ThreadMismatch {
                agent_id: self.state.snapshot.identity.id.clone(),
                expected: self.state.snapshot.identity.id.clone(),
                actual: thread_id,
            });
        }
        let current = self
            .runtime
            .thread_events
            .snapshot(thread_id.as_str())
            .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?;
        let projected = project_thread_facts(thread_id.as_str(), current.revision, facts);
        if projected.notifications.is_empty() {
            return Ok(());
        }

        let expected_revision = self.state.snapshot.revision;
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
                let agent_id = next.snapshot.identity.id.clone();
                self.state = next;
                self.runtime
                    .thread_events
                    .publish_batch(projected.notifications.clone())
                    .await
                    .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?;
                self.host
                    .observer()
                    .publish(AgentCommittedEvent {
                        agent_id,
                        thread_id: Some(thread_id),
                        turn_id: None,
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

fn find_inference<'a>(
    state: &'a super::super::ThreadActorState,
    inference_id: &str,
) -> Option<&'a pl_protocol::InferenceBillingRecord> {
    state
        .session
        .billing_by_turn
        .values()
        .flat_map(|billing| billing.inferences.iter())
        .find(|inference| inference.inference_id == inference_id)
}

fn append_inference(
    state: &mut super::super::ThreadActorState,
    turn_id: &TurnId,
    inference: &AgentInferenceCommit,
) -> AgentRuntimeResult<()> {
    state
        .session
        .billing_by_turn
        .entry(turn_id.to_string())
        .or_default()
        .append(inference.billing.clone())
        .map_err(AgentRuntimeError::InvalidInput)?;
    state.session.usage = state.session.billing_by_turn.values().fold(
        pl_model::TokenUsage::default(),
        |mut aggregate, billing| {
            let usage = billing.aggregate_usage();
            aggregate.prompt_tokens = aggregate.prompt_tokens.saturating_add(usage.prompt_tokens);
            aggregate.cached_prompt_tokens = aggregate
                .cached_prompt_tokens
                .saturating_add(usage.cached_prompt_tokens);
            aggregate.completion_tokens = aggregate
                .completion_tokens
                .saturating_add(usage.completion_tokens);
            aggregate.reasoning_tokens = aggregate
                .reasoning_tokens
                .saturating_add(usage.reasoning_tokens);
            aggregate.total_tokens = aggregate.total_tokens.saturating_add(usage.total_tokens);
            aggregate
        },
    );
    state.session.last_context_tokens = Some(inference.billing.normalized_usage.total_tokens);
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ThreadContextMutation;

    #[test]
    fn transcript_mutation_skips_unchanged_checkpoints() {
        let items = vec![item("first")];

        assert!(transcript_mutation(&items, &items).is_none());
    }

    #[test]
    fn transcript_mutation_appends_only_the_new_suffix() {
        let previous = vec![item("first")];
        let suffix = item("second");
        let next = vec![previous[0].clone(), suffix.clone()];

        let Some(ThreadContextMutation::Append { items }) = transcript_mutation(&previous, &next)
        else {
            panic!("expected append mutation");
        };
        assert_eq!(items, vec![suffix]);
    }

    #[test]
    fn transcript_mutation_replaces_compacted_or_rolled_back_history() {
        let previous = vec![item("first"), item("second")];
        let next = vec![item("compacted")];

        let Some(ThreadContextMutation::Replace { items }) = transcript_mutation(&previous, &next)
        else {
            panic!("expected replace mutation");
        };
        assert_eq!(items, next);
    }

    fn item(content: &str) -> crate::ModelContextItem {
        crate::ModelContextItem::from(crate::user_text_message(content))
    }
}
