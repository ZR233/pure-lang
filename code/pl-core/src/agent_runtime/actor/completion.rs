use super::super::execution::{TurnCompletion, add_usage, turn_outcome};
use super::super::{AgentActivityState, AgentRuntimeEventKind, AgentRuntimeHost};
use super::AgentActor;
use crate::agent_runtime::state::unix_timestamp;
use crate::session_event::compaction_observation;

impl<H> AgentActor<H>
where
    H: AgentRuntimeHost,
{
    pub(super) async fn finish_turn(&mut self, completion: TurnCompletion) {
        let Some(active) = &self.active else {
            return;
        };
        if active.turn_id != completion.turn_id
            || active.start_revision != completion.start_revision
            || active.dispatch_generation != completion.dispatch_generation
        {
            return;
        }
        if let Err(error) = self.flush_pending_traces().await {
            self.fault_in_memory(error.to_string());
            return;
        }
        if let Err(error) = self.flush_pending_observations().await {
            self.fault_in_memory(error.to_string());
            return;
        }
        let compactions = completion
            .result
            .as_ref()
            .ok()
            .map_or_else(Vec::new, |result| result.context_compactions.clone());
        for compaction in &compactions {
            if let Err(error) = self
                .persist_turn_observation(compaction_observation(compaction, unix_timestamp()))
                .await
            {
                self.fault_in_memory(error.to_string());
                return;
            }
        }
        let active = self
            .active
            .take()
            .expect("validated active turn must still be present");
        let cancelled = active.cancellation_requested || completion.cancelled;
        let finalized_with_tool = completion.finalized_with_tool;
        let (outcome, _, result) = turn_outcome(
            active.turn_id.clone(),
            active.session_id.clone(),
            completion.result,
            cancelled,
        );
        let finalized_with_tool = (outcome.kind == super::super::TurnOutcomeKind::Completed)
            .then_some(finalized_with_tool)
            .flatten();
        let mut next = self.state.clone();
        if let Some(session) = next.sessions.get_mut(&active.session_id) {
            session.trace_sequence = session.trace_sequence.max(completion.next_trace_sequence);
            if let Some(completed_session) = completion.session {
                session.session = completed_session;
            }
            if let Some(result) = &result {
                add_usage(&mut session.usage, &result.usage);
                session.last_context_tokens = result.last_context_tokens;
            }
        }
        next.snapshot.active_turn_id = None;
        next.snapshot.active_session_id = None;
        for input in &mut next.pending_inputs {
            if !matches!(
                &input.delivery_state,
                super::super::MailboxDeliveryState::Claimed { turn_id, .. }
                    if turn_id == &active.turn_id
            ) {
                continue;
            }
            input.delivery_state = super::super::MailboxDeliveryState::Pending;
            if input.turn_id == active.turn_id {
                input.turn_id = super::super::TurnId::generate();
            }
        }
        next.active_input = None;
        next.refresh_mailbox_snapshot();
        next.snapshot.last_turn = Some(outcome.clone());
        next.snapshot.mailbox_delivery_phase = super::super::MailboxDeliveryPhase::NextTurn;
        next.snapshot.activity = if next.has_triggering_input() {
            AgentActivityState::Queued
        } else {
            AgentActivityState::Idle
        };
        let committed = self
            .commit_transition(next, Vec::new(), |snapshot| {
                AgentRuntimeEventKind::TurnFinished {
                    outcome: outcome.clone(),
                    snapshot,
                    finalized_with_tool: finalized_with_tool.clone(),
                }
            })
            .await;
        if let Err(error) = committed {
            self.fault_in_memory(error.to_string());
            return;
        }
        if self.dispatch_enabled && self.state.has_triggering_input() {
            self.begin_next_turn().await;
        }
    }
}
