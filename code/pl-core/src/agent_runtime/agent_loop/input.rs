use super::super::state::{AgentRuntimeError, unix_timestamp};
use super::super::{
    AgentCommand, AgentCurrentSessionSubmitRequest, AgentInteractionContinuationRequest,
    AgentRuntimeEventKind, AgentRuntimeHost, AgentRuntimeResult, AgentSnapshotTransition,
    AgentSubmitRequest, AgentTurnSubmitPolicy, DurableMailboxEnvelope, TurnId,
};
use super::AgentLoop;

impl<H> AgentLoop<H>
where
    H: AgentRuntimeHost,
{
    pub(super) async fn submit(
        &mut self,
        request: AgentSubmitRequest,
    ) -> AgentRuntimeResult<TurnId> {
        if !self.state.snapshot.state.is_accepting_work() {
            return Err(AgentRuntimeError::NotActive(
                self.state.snapshot.identity.id.clone(),
                self.state.snapshot.state.clone(),
            ));
        }
        if self.state.snapshot.identity.id != request.thread_id {
            return Err(AgentRuntimeError::ThreadMismatch {
                agent_id: self.state.snapshot.identity.id.clone(),
                expected: self.state.snapshot.identity.id.clone(),
                actual: request.thread_id,
            });
        }
        if let Some(existing) = request.mail_id.as_deref().and_then(|mail_id| {
            self.state
                .active_input
                .iter()
                .chain(self.state.pending_inputs.iter())
                .find(|input| input.mail_id == mail_id)
                .map(|input| input.turn_id.clone())
        }) {
            return Ok(existing);
        }
        let live_turn = self.active.as_ref().and_then(|active| {
            (request.turn_policy != AgentTurnSubmitPolicy::StartOrQueue
                && !active.is_cancelling()
                && active.thread_id == request.thread_id)
                .then(|| {
                    (
                        active.turn_id.clone(),
                        active.steer_sender.clone(),
                        active.budget_refresh.clone(),
                    )
                })
        });
        match request.turn_policy {
            AgentTurnSubmitPolicy::StartOnly if self.active.is_some() => {
                return Err(AgentRuntimeError::InvalidInput(
                    "startTurn requires an idle Thread".to_string(),
                ));
            }
            AgentTurnSubmitPolicy::SteerOnly if live_turn.is_none() => {
                return Err(AgentRuntimeError::InvalidInput(
                    "steerTurn requires an active Turn".to_string(),
                ));
            }
            AgentTurnSubmitPolicy::StartOrSteer
            | AgentTurnSubmitPolicy::StartOrQueue
            | AgentTurnSubmitPolicy::StartOnly
            | AgentTurnSubmitPolicy::SteerOnly => {}
        }
        let turn_id = live_turn
            .as_ref()
            .map_or_else(TurnId::generate, |(turn_id, _, _)| turn_id.clone());
        let mail_id = request
            .mail_id
            .unwrap_or_else(|| format!("mail:{}", TurnId::generate()));
        let mut next = self.state.clone();
        let mut input = DurableMailboxEnvelope {
            mail_id,
            turn_id: turn_id.clone(),
            thread_id: request.thread_id,
            payload: request.payload,
            queue_coalescing_key: request.queue_coalescing_key,
            budget_action: request.budget_action,
            delivery_state: Default::default(),
            queued_at: unix_timestamp(),
        };
        if live_turn.is_some() {
            input
                .claim(turn_id.clone())
                .map_err(|error| AgentRuntimeError::InvalidInput(error.to_string()))?;
        }
        next.pending_inputs.push_back(input.clone());
        next.refresh_mailbox_snapshot();
        if live_turn.is_none() && next.snapshot.state.is_idle() {
            let queued_turn_id = next
                .triggering_turn_id()
                .expect("the newly appended input must make the mailbox triggerable");
            next.snapshot
                .transition(AgentCommand::Queue {
                    turn_id: queued_turn_id,
                })
                .map_err(|error| AgentRuntimeError::InvalidInput(error.to_string()))?;
        }
        self.commit_transition(super::persist::TransitionCommit::new(next), |snapshot| {
            AgentRuntimeEventKind::TurnQueued {
                input: input.clone(),
                snapshot: Box::new(snapshot),
            }
        })
        .await?;
        if let Some((_, steer_sender, budget_refresh)) = live_turn {
            if steer_sender.send(input.clone()).is_err() {
                self.release_undelivered_steer(&input.mail_id).await?;
            } else if input.budget_action == super::super::MailboxBudgetAction::Refresh {
                budget_refresh.refresh();
            }
            return Ok(turn_id);
        }
        self.dispatch_enabled = true;
        if self.active.is_none() && self.dispatch_enabled && self.state.has_triggering_input() {
            self.begin_next_turn().await;
        }
        Ok(turn_id)
    }

    async fn release_undelivered_steer(&mut self, mail_id: &str) -> AgentRuntimeResult<()> {
        let mut next = self.state.clone();
        let mut released = false;
        for input in &mut next.pending_inputs {
            if input.mail_id != mail_id || !input.delivery_state.is_claimed() {
                continue;
            }
            input
                .requeue(TurnId::generate())
                .map_err(|error| AgentRuntimeError::InvalidInput(error.to_string()))?;
            released = true;
            break;
        }
        if !released {
            return Ok(());
        }
        next.refresh_mailbox_snapshot();
        self.commit_transition(
            super::persist::TransitionCommit::new(next).settlement(),
            |snapshot| AgentRuntimeEventKind::StateChanged {
                snapshot: Box::new(snapshot),
            },
        )
        .await
    }

    pub(super) async fn submit_current_session(
        &mut self,
        root_agent_id: super::super::ThreadId,
        request: AgentCurrentSessionSubmitRequest,
    ) -> AgentRuntimeResult<TurnId> {
        if !self.state.snapshot.state.is_accepting_work() {
            return Err(AgentRuntimeError::NotActive(
                self.state.snapshot.identity.id.clone(),
                self.state.snapshot.state.clone(),
            ));
        }
        debug_assert!(
            root_agent_id == self.state.snapshot.identity.id
                || self.state.snapshot.identity.depth > 0
        );
        self.submit(AgentSubmitRequest {
            thread_id: self.state.snapshot.identity.id.clone(),
            payload: request.payload,
            queue_coalescing_key: None,
            mail_id: request.mail_id,
            turn_policy: AgentTurnSubmitPolicy::StartOrSteer,
            budget_action: request.budget_action,
        })
        .await
    }

    pub(super) async fn submit_interaction_continuation(
        &mut self,
        root_agent_id: super::super::ThreadId,
        request: AgentInteractionContinuationRequest,
    ) -> AgentRuntimeResult<()> {
        if !self.state.snapshot.state.is_accepting_work() {
            return Err(AgentRuntimeError::NotActive(
                self.state.snapshot.identity.id.clone(),
                self.state.snapshot.state.clone(),
            ));
        }
        debug_assert!(
            root_agent_id == self.state.snapshot.identity.id
                || self.state.snapshot.identity.depth > 0
        );
        if request.interaction.status() != pl_protocol::InteractionStatus::Resolved
            || request.interaction.resolution().is_none()
        {
            return Err(AgentRuntimeError::InvalidInput(
                "interaction continuation requires a resolved interaction".to_string(),
            ));
        }
        let mail_id = request
            .input
            .mail_id
            .clone()
            .filter(|mail_id| !mail_id.trim().is_empty())
            .ok_or_else(|| {
                AgentRuntimeError::InvalidInput(
                    "interaction continuation requires a stable mail id".to_string(),
                )
            })?;
        let expected_mail_id = AgentInteractionContinuationRequest::stable_mail_id(
            &request.interaction.interaction_id,
        );
        if mail_id != expected_mail_id {
            return Err(AgentRuntimeError::InvalidInput(format!(
                "interaction continuation mail id must be {expected_mail_id}"
            )));
        }
        let thread_id = self.state.snapshot.identity.id.clone();
        let canonical = self
            .runtime
            .thread_events
            .snapshot(thread_id.as_str())
            .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?;
        let existing_input = self
            .state
            .active_input
            .iter()
            .chain(self.state.pending_inputs.iter())
            .find(|input| input.mail_id == mail_id);
        let canonical_interaction = canonical
            .interactions
            .iter()
            .find(|candidate| candidate.interaction_id == request.interaction.interaction_id);
        let Some(canonical_interaction) = canonical_interaction else {
            if existing_input.is_some() {
                return Ok(());
            }
            return Err(AgentRuntimeError::InvalidInput(
                "interaction continuation has no canonical interaction".to_string(),
            ));
        };
        validate_interaction_identity(canonical_interaction, &request.interaction)?;
        match canonical_interaction.status() {
            pl_protocol::InteractionStatus::Resolved => {
                if canonical_interaction.resolution() == request.interaction.resolution() {
                    return Ok(());
                }
                return Err(AgentRuntimeError::InvalidInput(
                    "interaction was already resolved with a different resolution".to_string(),
                ));
            }
            pl_protocol::InteractionStatus::Pending => {}
            pl_protocol::InteractionStatus::Cancelled | pl_protocol::InteractionStatus::Expired => {
                return Err(AgentRuntimeError::InvalidInput(
                    "interaction continuation requires a pending canonical interaction".to_string(),
                ));
            }
        }
        if existing_input.is_some() {
            return Err(AgentRuntimeError::InvalidInput(
                "pending interaction already has a continuation input".to_string(),
            ));
        }

        let turn_id = TurnId::generate();
        let input = DurableMailboxEnvelope {
            mail_id,
            turn_id: turn_id.clone(),
            thread_id: thread_id.clone(),
            payload: request.input.payload,
            queue_coalescing_key: None,
            budget_action: request.input.budget_action,
            delivery_state: Default::default(),
            queued_at: unix_timestamp(),
        };
        let mut next = self.state.clone();
        if let Some(plan) = crate::session::plan::state_for_resolved_interaction(
            next.session.session.plan(),
            &request.interaction,
        )
        .map_err(AgentRuntimeError::InvalidInput)?
        {
            next.session.session.replace_plan(Some(plan));
        }
        next.pending_inputs.push_back(input.clone());
        next.refresh_mailbox_snapshot();
        let waiting_interaction_id = match &next.snapshot.state {
            super::super::AgentState::WaitingInteraction(waiting) => {
                Some(waiting.interaction_id().to_string())
            }
            super::super::AgentState::Idle(_)
            | super::super::AgentState::Queued(_)
            | super::super::AgentState::Running(_)
            | super::super::AgentState::WaitingTool(_)
            | super::super::AgentState::Cancelling(_)
            | super::super::AgentState::Closing(_)
            | super::super::AgentState::Closed(_)
            | super::super::AgentState::Faulted(_) => None,
        };
        if let Some(waiting_interaction_id) = waiting_interaction_id {
            if waiting_interaction_id != request.interaction.interaction_id {
                return Err(AgentRuntimeError::InvalidInput(format!(
                    "agent is waiting for interaction {waiting_interaction_id}, not {}",
                    request.interaction.interaction_id
                )));
            }
            next.snapshot
                .transition(AgentCommand::ContinueInteraction {
                    interaction_id: waiting_interaction_id,
                    turn_id: next
                        .triggering_turn_id()
                        .expect("continuation input must be pending"),
                })
                .map_err(|error| AgentRuntimeError::InvalidInput(error.to_string()))?;
        } else if next.snapshot.state.is_idle() {
            next.snapshot
                .transition(AgentCommand::Queue {
                    turn_id: turn_id.clone(),
                })
                .map_err(|error| AgentRuntimeError::InvalidInput(error.to_string()))?;
        }
        let interaction = request.interaction;
        self.commit_transition(
            super::persist::TransitionCommit::new(next).with_thread_facts(vec![
                crate::ThreadNotificationFact::durable(
                    interaction.updated_at,
                    pl_protocol::ThreadNotification::InteractionChanged {
                        interaction: Box::new(interaction),
                    },
                ),
            ]),
            |snapshot| AgentRuntimeEventKind::TurnQueued {
                input: input.clone(),
                snapshot: Box::new(snapshot),
            },
        )
        .await?;
        self.dispatch_enabled = true;
        if self.active.is_none() && self.state.has_triggering_input() {
            self.begin_next_turn().await;
        }
        Ok(())
    }
}

fn validate_interaction_identity(
    canonical: &pl_protocol::InteractionRequest,
    resolved: &pl_protocol::InteractionRequest,
) -> AgentRuntimeResult<()> {
    if canonical.same_request(resolved) {
        return Ok(());
    }
    Err(AgentRuntimeError::InvalidInput(
        "interaction continuation does not match the canonical interaction".to_string(),
    ))
}
