use super::super::state::{AgentRuntimeError, unix_timestamp};
use super::super::{
    AgentCurrentSessionSubmitRequest, AgentInteractionContinuationRequest, AgentLifecycleState,
    AgentRuntimeEventKind, AgentRuntimeHost, AgentRuntimeResult, AgentSubmitRequest,
    AgentTurnSubmitPolicy, DurableMailboxEnvelope, MailboxDeliveryState, TurnId,
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
        if self.state.snapshot.lifecycle != AgentLifecycleState::Active {
            return Err(AgentRuntimeError::NotActive(
                self.state.snapshot.identity.id.clone(),
                self.state.snapshot.lifecycle,
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
                && !active.cancelling
                && active.thread_id == request.thread_id)
                .then(|| (active.turn_id.clone(), active.steer_sender.clone()))
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
            .map_or_else(TurnId::generate, |(turn_id, _)| turn_id.clone());
        let mail_id = request
            .mail_id
            .unwrap_or_else(|| format!("mail:{}", TurnId::generate()));
        let mut next = self.state.clone();
        let mut input = DurableMailboxEnvelope {
            mail_id,
            turn_id: turn_id.clone(),
            thread_id: request.thread_id,
            message: request.message,
            presentation: request.presentation,
            metadata: request.metadata,
            queue_coalescing_key: request.queue_coalescing_key,
            delivery_state: Default::default(),
            queued_at: unix_timestamp(),
        };
        if live_turn.is_some() {
            input.claim(turn_id.clone());
        }
        next.pending_inputs.push_back(input.clone());
        next.refresh_mailbox_snapshot();
        self.commit_transition(next, Vec::new(), |snapshot| {
            AgentRuntimeEventKind::TurnQueued {
                input: input.clone(),
                snapshot,
            }
        })
        .await?;
        if let Some((_, steer_sender)) = live_turn {
            if steer_sender.send(input.clone()).is_err() {
                self.release_undelivered_steer(&input.mail_id).await?;
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
            if input.mail_id != mail_id
                || !matches!(input.delivery_state, MailboxDeliveryState::Claimed { .. })
            {
                continue;
            }
            input.delivery_state = MailboxDeliveryState::Pending;
            input.turn_id = TurnId::generate();
            released = true;
            break;
        }
        if !released {
            return Ok(());
        }
        next.refresh_mailbox_snapshot();
        self.commit_transition(next, Vec::new(), |snapshot| {
            AgentRuntimeEventKind::StateChanged { snapshot }
        })
        .await
    }

    pub(super) async fn submit_current_session(
        &mut self,
        root_agent_id: super::super::AgentId,
        request: AgentCurrentSessionSubmitRequest,
    ) -> AgentRuntimeResult<TurnId> {
        if self.state.snapshot.lifecycle != AgentLifecycleState::Active {
            return Err(AgentRuntimeError::NotActive(
                self.state.snapshot.identity.id.clone(),
                self.state.snapshot.lifecycle,
            ));
        }
        debug_assert!(
            root_agent_id == self.state.snapshot.identity.id
                || self.state.snapshot.identity.depth > 0
        );
        self.submit(AgentSubmitRequest {
            thread_id: self.state.snapshot.identity.id.clone(),
            message: request.message,
            presentation: request.presentation,
            metadata: request.metadata,
            queue_coalescing_key: None,
            mail_id: request.mail_id,
            turn_policy: AgentTurnSubmitPolicy::StartOrSteer,
        })
        .await
    }

    pub(super) async fn submit_interaction_continuation(
        &mut self,
        root_agent_id: super::super::AgentId,
        request: AgentInteractionContinuationRequest,
    ) -> AgentRuntimeResult<()> {
        if self.state.snapshot.lifecycle != AgentLifecycleState::Active {
            return Err(AgentRuntimeError::NotActive(
                self.state.snapshot.identity.id.clone(),
                self.state.snapshot.lifecycle,
            ));
        }
        debug_assert!(
            root_agent_id == self.state.snapshot.identity.id
                || self.state.snapshot.identity.depth > 0
        );
        if request.interaction.status != pl_protocol::InteractionStatus::Resolved
            || request.interaction.resolution.is_none()
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
        match canonical_interaction.status {
            pl_protocol::InteractionStatus::Resolved => {
                if canonical_interaction.resolution == request.interaction.resolution {
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
            message: request.input.message,
            presentation: request.input.presentation,
            metadata: request.input.metadata,
            queue_coalescing_key: None,
            delivery_state: Default::default(),
            queued_at: unix_timestamp(),
        };
        let mut next = self.state.clone();
        next.pending_inputs.push_back(input.clone());
        next.refresh_mailbox_snapshot();
        let interaction = request.interaction;
        self.commit_transition_with_thread_facts(
            next,
            Vec::new(),
            vec![crate::ThreadNotificationFact::durable(
                interaction.updated_at,
                pl_protocol::ThreadNotification::InteractionChanged {
                    interaction: Box::new(interaction),
                },
            )],
            None,
            |snapshot| AgentRuntimeEventKind::TurnQueued {
                input: input.clone(),
                snapshot,
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
    if canonical.kind == resolved.kind
        && canonical.scope == resolved.scope
        && canonical.payload == resolved.payload
        && canonical.created_at == resolved.created_at
    {
        return Ok(());
    }
    Err(AgentRuntimeError::InvalidInput(
        "interaction continuation does not match the canonical interaction".to_string(),
    ))
}
