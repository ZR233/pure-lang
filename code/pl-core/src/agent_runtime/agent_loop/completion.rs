use super::super::{AgentRuntimeEventKind, AgentRuntimeHost, AgentSnapshotTransition};
use super::AgentLoop;
use super::running_turn::{TurnCompletion, TurnSessionDisposition, add_usage, turn_outcome};
use crate::agent_runtime::state::unix_timestamp;
use crate::thread_event::compaction_observation;

impl<H> AgentLoop<H>
where
    H: AgentRuntimeHost,
{
    pub(super) async fn finish_turn(&mut self, completion: TurnCompletion) {
        let Some(active) = self.active.as_ref() else {
            return;
        };
        if active.turn_id != completion.turn_id
            || active.start_revision != completion.start_revision
            || !std::sync::Arc::ptr_eq(&active.identity, &completion.identity)
        {
            return;
        }
        if let Err(error) = self.flush_pending_traces().await {
            self.mark_projection_failure(&error);
            tracing::error!(
                agent_id = %self.state.snapshot.identity.id,
                error = %error,
                "pending trace projection was rejected while settling the turn"
            );
        }
        if let Err(error) = self.flush_pending_observations().await {
            self.mark_projection_failure(&error);
            tracing::error!(
                agent_id = %self.state.snapshot.identity.id,
                error = %error,
                "pending observation projection was rejected while settling the turn"
            );
        }
        let compactions = completion
            .worker_outcome
            .returned()
            .map_or_else(Vec::new, |result| result.context_compactions.clone());
        for compaction in &compactions {
            if let Err(error) = self
                .persist_turn_observation(compaction_observation(compaction, unix_timestamp()))
                .await
            {
                self.mark_projection_failure(&error);
                tracing::error!(
                    agent_id = %self.state.snapshot.identity.id,
                    error = %error,
                    "compaction observation was rejected while settling the turn"
                );
            }
        }
        let active = self
            .active
            .take()
            .expect("validated active turn must still be present");
        let finalization = turn_outcome(
            active.turn_id.clone(),
            active.thread_id.clone(),
            active.terminal(completion.worker_outcome),
            Some(active.started_at),
        );
        let outcome = finalization.outcome;
        let result = finalization.retained_result;
        let mut next = self.state.clone();
        next.session.trace_sequence = next
            .session
            .trace_sequence
            .max(completion.next_trace_sequence);
        match completion.session {
            TurnSessionDisposition::Preserve => {}
            TurnSessionDisposition::Replace(completed_session) => {
                next.session.session = completed_session;
            }
        }
        if let Some(result) = result.as_ref() {
            if !next
                .session
                .billing_by_turn
                .contains_key(active.turn_id.as_str())
            {
                add_usage(&mut next.session.usage, &result.usage);
            }
            if result.last_context_tokens.is_some() {
                next.session.last_context_tokens = result.last_context_tokens;
            }
        }
        for input in &mut next.pending_inputs {
            if !matches!(
                &input.delivery_state,
                super::super::MailboxDeliveryState::Claimed(state)
                    if state.turn_id() == &active.turn_id
            ) {
                continue;
            }
            let turn_id = if input.turn_id == active.turn_id {
                super::super::TurnId::generate()
            } else {
                input.turn_id.clone()
            };
            if let Err(error) = input.requeue(turn_id) {
                self.fault(error.to_string()).await;
                return;
            }
        }
        let next_turn_id = next.triggering_turn_id();
        let waiting_for_interaction = outcome.outcome.is_interaction_boundary()
            && matches!(
                &next.snapshot.state,
                super::super::AgentState::WaitingInteraction(state)
                    if state.turn_id() == &active.turn_id
            );
        if !waiting_for_interaction
            && let Err(error) = next
                .snapshot
                .transition(super::super::AgentCommand::Settle { next_turn_id })
        {
            self.fault(error.to_string()).await;
            return;
        }
        next.active_input = None;
        next.refresh_mailbox_snapshot();
        next.snapshot.last_turn = Some(outcome.clone());
        let committed = self
            .commit_transition(super::persist::TransitionCommit::new(next), |snapshot| {
                AgentRuntimeEventKind::TurnFinished {
                    outcome: outcome.clone(),
                    snapshot: Box::new(snapshot),
                }
            })
            .await;
        if let Err(error) = committed {
            self.fault(error.to_string()).await;
            return;
        }
        if !waiting_for_interaction && self.dispatch_enabled && self.state.has_triggering_input() {
            self.begin_next_turn().await;
        }
    }
}
