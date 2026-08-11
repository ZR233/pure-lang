use super::super::{AgentRuntimeEventKind, AgentRuntimeHost};
use super::AgentLoop;
use super::running_turn::{TurnCompletion, add_usage, turn_outcome};
use crate::agent_runtime::state::unix_timestamp;
use crate::thread_event::compaction_observation;

impl<H> AgentLoop<H>
where
    H: AgentRuntimeHost,
{
    pub(super) async fn finish_turn(&mut self, completion: TurnCompletion) {
        let Some(active) = &self.active else {
            return;
        };
        if active.turn_id != completion.turn_id
            || active.start_revision != completion.start_revision
            || !std::sync::Arc::ptr_eq(&active.identity, &completion.identity)
        {
            return;
        }
        if let Err(error) = self.flush_pending_traces().await {
            self.fault(error.to_string()).await;
            return;
        }
        if let Err(error) = self.flush_pending_observations().await {
            self.fault(error.to_string()).await;
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
                self.fault(error.to_string()).await;
                return;
            }
        }
        let active = self
            .active
            .take()
            .expect("validated active turn must still be present");
        let cancelled = active.cancelling || completion.cancelled;
        let finalized_with_tool = completion.finalized_with_tool;
        let (outcome, _, result) = turn_outcome(
            active.turn_id.clone(),
            active.thread_id.clone(),
            completion.result,
            cancelled,
        );
        let finalized_with_tool = (outcome.kind == super::super::TurnOutcomeKind::Completed)
            .then_some(finalized_with_tool)
            .flatten();
        let mut next = self.state.clone();
        next.session.trace_sequence = next
            .session
            .trace_sequence
            .max(completion.next_trace_sequence);
        if let Some(completed_session) = completion.session {
            next.session.session = completed_session;
        }
        if let Some(result) = &result {
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
        next.snapshot.active_turn_id = None;
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
            self.fault(error.to_string()).await;
            return;
        }
        if self.dispatch_enabled && self.state.has_triggering_input() {
            self.begin_next_turn().await;
        }
    }
}
