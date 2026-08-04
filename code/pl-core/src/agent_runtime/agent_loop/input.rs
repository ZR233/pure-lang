use super::super::state::{AgentRuntimeError, unix_timestamp};
use super::super::{
    AgentActivityState, AgentCurrentSessionSubmitRequest, AgentLifecycleState,
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
            (!active.cancelling && active.thread_id == request.thread_id)
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
            delivery_state: Default::default(),
            queued_at: unix_timestamp(),
        };
        if live_turn.is_some() {
            input.claim(turn_id.clone());
        }
        next.pending_inputs.push_back(input.clone());
        next.refresh_mailbox_snapshot();
        if self.active.is_none() && next.has_triggering_input() {
            next.snapshot.activity = AgentActivityState::Queued;
        }
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
            mail_id: request.mail_id,
            turn_policy: AgentTurnSubmitPolicy::StartOrSteer,
        })
        .await
    }
}
