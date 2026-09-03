use std::collections::BTreeSet;

use super::super::host::{ThreadProjectionCommit, transcript_mutation};
use super::super::state::{AgentRuntimeError, unix_timestamp};
use super::super::{
    AgentActivityUpdate, AgentCommand, AgentInferenceCommit, AgentProgressCheckpoint,
    AgentProgressReport, AgentProgressStage, AgentRuntimeEventKind, AgentRuntimeHost,
    AgentRuntimeResult, AgentSnapshotTransition, AgentTurnCheckpoint, DurableCommitFacts,
    MailboxDeliveryState, ThreadId, ThreadMutation, TurnId,
};
use super::AgentLoop;
use super::commit::{CommitPublication, PendingCommit};
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
        activity: AgentActivityUpdate,
    ) -> AgentRuntimeResult<()> {
        let Some(active) = &mut self.active else {
            return Ok(());
        };
        if active.turn_id != turn_id
            || !self.state.snapshot.state.is_operational()
            || active.activity == activity
        {
            return Ok(());
        }
        let previous_activity = active.activity.clone();
        let thread_id = active.thread_id.clone();
        active.activity = activity.clone();
        let command = match &activity {
            AgentActivityUpdate::Running => AgentCommand::Resume {
                turn_id: turn_id.clone(),
            },
            AgentActivityUpdate::WaitingTool => AgentCommand::WaitForTool {
                turn_id: turn_id.clone(),
            },
            AgentActivityUpdate::WaitingInteraction { interaction_id } => {
                AgentCommand::WaitForInteraction {
                    turn_id: turn_id.clone(),
                    interaction_id: interaction_id.clone(),
                }
            }
        };
        let mut next = self.state.clone();
        next.snapshot
            .transition(command)
            .map_err(|error| AgentRuntimeError::InvalidInput(error.to_string()))?;
        let result = self
            .commit_transition(super::persist::TransitionCommit::new(next), |snapshot| {
                AgentRuntimeEventKind::TurnActivityChanged {
                    turn_id: turn_id.clone(),
                    thread_id,
                    activity: activity.clone(),
                    snapshot: Box::new(snapshot),
                }
            })
            .await;
        if result.is_err()
            && let Some(active) = self.active.as_mut()
            && active.turn_id == turn_id
        {
            active.activity = previous_activity;
        }
        result
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
            || active.is_cancelling()
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
            && active_input.delivery_state.is_claimed()
        {
            active_input
                .consume(checkpoint.sequence)
                .map_err(|error| AgentRuntimeError::InvalidInput(error.to_string()))?;
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
                        MailboxDeliveryState::Claimed(state)
                            if state.turn_id() == &checkpoint.turn_id
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
        let workflow_changed =
            self.state.session.session.workflow() != checkpoint.session.workflow();
        let workflow_projection = checkpoint
            .session
            .workflow()
            .map(pl_protocol::WorkflowRuntimeSnapshot::from);
        next.session.session = checkpoint.session;
        let projection = if checkpoint.inference.is_some() || workflow_changed {
            let mut current = self
                .runtime
                .thread_events
                .snapshot(checkpoint.thread_id.as_str())
                .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?;
            let mut notifications = Vec::new();
            if let Some(inference) = checkpoint.inference.as_ref() {
                append_inference(&mut next, &checkpoint.turn_id, inference)?;
                let projected = project_observation(
                    checkpoint.thread_id.as_str(),
                    checkpoint.turn_id.as_str(),
                    current.revision,
                    &current,
                    TurnObservation::RuntimeDelta(Box::new(inference.runtime_delta.clone())),
                );
                let projected_thread = self
                    .runtime
                    .thread_events
                    .project(checkpoint.thread_id.as_str(), &projected.notifications)
                    .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?;
                notifications.extend(projected_thread.notifications);
                current = projected_thread.snapshot;
            }
            if workflow_changed {
                let mut runtime = current.runtime.clone().unwrap_or_else(|| {
                    crate::thread_event::empty_runtime(checkpoint.thread_id.as_str())
                });
                runtime.workflow = workflow_projection;
                runtime.updated_at = next.snapshot.updated_at;
                let projected = project_thread_facts(
                    checkpoint.thread_id.as_str(),
                    &current,
                    vec![ThreadNotificationFact::durable(
                        next.snapshot.updated_at,
                        pl_protocol::ThreadNotification::ThreadRuntimeUpdated {
                            runtime: Box::new(runtime),
                        },
                    )],
                );
                let projected_thread = self
                    .runtime
                    .thread_events
                    .project(checkpoint.thread_id.as_str(), &projected.notifications)
                    .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?;
                notifications.extend(projected_thread.notifications);
                current = projected_thread.snapshot;
            }
            next.session.thread_revision = current.revision;
            Some(ThreadProjectionCommit {
                snapshot: current,
                notifications,
            })
        } else {
            None
        };
        let publication = projection.as_ref().map(|projection| {
            CommitPublication::new(
                Some(checkpoint.thread_id.clone()),
                Some(checkpoint.turn_id.clone()),
            )
            .with_thread_notifications(projection.notifications.clone())
        });
        let mut facts =
            DurableCommitFacts::from_state(&next, Vec::new(), Vec::new(), projection, context);
        facts.inference = checkpoint.inference.clone();
        let pending = PendingCommit::new(
            next,
            facts,
            ThreadMutation::ReplaceThread {
                thread_id: checkpoint.thread_id.clone(),
            },
        );
        let pending = match publication {
            Some(publication) => pending.publish(publication),
            None => pending,
        };
        self.commit_and_publish(pending).await?;
        tracing::trace!(
            agent_id = %self.state.snapshot.identity.id,
            turn_id = %checkpoint.turn_id,
            sequence = checkpoint.sequence,
            reason = ?checkpoint.reason,
            "agent turn checkpoint committed"
        );
        if let Some(active) = &mut self.active {
            active.checkpoint_sequence = checkpoint.sequence;
        }
        Ok(())
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
        let mut plan_state = self.state.session.session.plan().cloned();
        let mut plan_changed = false;
        for fact in &facts {
            if let pl_protocol::ThreadNotification::InteractionChanged { interaction } =
                &fact.notification
                && interaction.status() == pl_protocol::InteractionStatus::Pending
                && let Some(next_plan) = crate::session::plan::state_for_pending_interaction(
                    plan_state.as_ref(),
                    interaction,
                )
                .map_err(AgentRuntimeError::InvalidInput)?
            {
                plan_state = Some(next_plan);
                plan_changed = true;
            }
        }
        let recovered_wait = (self.active.is_none()
            && self.state.snapshot.state.is_idle()
            && self.state.pending_inputs.is_empty())
        .then(|| {
            facts.iter().find_map(|fact| match &fact.notification {
                pl_protocol::ThreadNotification::InteractionChanged { interaction }
                    if interaction.status() == pl_protocol::InteractionStatus::Pending =>
                {
                    Some((
                        interaction.scope.turn_id.clone(),
                        interaction.interaction_id.clone(),
                    ))
                }
                pl_protocol::ThreadNotification::TurnStarted { .. }
                | pl_protocol::ThreadNotification::TurnUpdated { .. }
                | pl_protocol::ThreadNotification::TurnCompleted { .. }
                | pl_protocol::ThreadNotification::ItemStarted { .. }
                | pl_protocol::ThreadNotification::ItemDelta { .. }
                | pl_protocol::ThreadNotification::ItemCompleted { .. }
                | pl_protocol::ThreadNotification::InteractionChanged { .. }
                | pl_protocol::ThreadNotification::ThreadRuntimeUpdated { .. }
                | pl_protocol::ThreadNotification::Lagged { .. } => None,
            })
        })
        .flatten();
        let current = self
            .runtime
            .thread_events
            .snapshot(thread_id.as_str())
            .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?;
        let projected = project_thread_facts(thread_id.as_str(), &current, facts);
        if projected.notifications.is_empty() {
            return Ok(());
        }

        let expected_revision = self.state.snapshot.revision;
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
        if plan_changed {
            next.session.session.replace_plan(plan_state);
        }
        if let Some((turn_id, interaction_id)) = recovered_wait {
            next.snapshot
                .transition(AgentCommand::RecoverWaitingInteraction {
                    turn_id: TurnId::new(turn_id)
                        .map_err(|error| AgentRuntimeError::InvalidInput(error.to_string()))?,
                    interaction_id,
                })
                .map_err(|error| AgentRuntimeError::InvalidInput(error.to_string()))?;
        }
        next.snapshot.revision = expected_revision.saturating_add(1);
        next.snapshot.updated_at = unix_timestamp();
        next.session.thread_revision = projected.through_revision;
        let durable_facts =
            DurableCommitFacts::from_state(&next, Vec::new(), Vec::new(), Some(projection), None);
        self.commit_and_publish(
            PendingCommit::new(
                next,
                durable_facts,
                ThreadMutation::AppendThreadNotifications {
                    thread_id: thread_id.clone(),
                },
            )
            .publish(
                CommitPublication::new(Some(thread_id), None)
                    .with_thread_notifications(projected.notifications),
            ),
        )
        .await
    }

    pub(super) async fn report_progress(
        &mut self,
        stage: AgentProgressStage,
        summary: String,
        next_step: String,
        detail: Option<String>,
    ) -> AgentRuntimeResult<AgentProgressCheckpoint> {
        let summary = bounded_required_text("summary", summary, 1_200)?;
        let next_step = bounded_required_text("nextStep", next_step, 500)?;
        let detail = bounded_optional_text("detail", detail, 20_000)?;
        // 携带 detail 的提交总是记录（实质报告内容）；仅短字段相同时才去重。
        let unchanged = detail.is_none()
            && self
                .state
                .snapshot
                .progress
                .as_ref()
                .is_some_and(|current| {
                    current.report.stage == stage
                        && current.report.summary == summary
                        && current.report.next_step == next_step
                });
        if unchanged {
            return Ok(self.state.snapshot.progress.clone().expect("checked above"));
        }
        let revision = self
            .state
            .snapshot
            .progress
            .as_ref()
            .map_or(1, |progress| progress.report.revision.saturating_add(1));
        let created_at = unix_timestamp();
        let report = AgentProgressReport {
            stage,
            summary,
            next_step,
            revision,
        };
        let checkpoint = AgentProgressCheckpoint {
            report: report.clone(),
            updated_at: created_at,
        };
        let submission = super::super::ProgressSubmissionCommit {
            report,
            detail,
            created_at,
        };
        let mut next = self.state.clone();
        next.snapshot.progress = Some(checkpoint.clone());
        self.commit_transition(
            super::persist::TransitionCommit::new(next).with_submission(submission),
            |snapshot| AgentRuntimeEventKind::StateChanged {
                snapshot: Box::new(snapshot),
            },
        )
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
        pl_protocol::TokenUsage::default(),
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

fn bounded_optional_text(
    field: &str,
    value: Option<String>,
    max_chars: usize,
) -> AgentRuntimeResult<Option<String>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim().to_string();
    if value.is_empty() {
        return Ok(None);
    }
    if value.chars().count() > max_chars {
        return Err(AgentRuntimeError::InvalidInput(format!(
            "{field} exceeds {max_chars} characters"
        )));
    }
    Ok(Some(value))
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
