use pl_protocol::{TurnCompletion, TurnOutcome};
use pl_trace::AgentEvent;

use crate::context_compaction::ContextCompactionSnapshot;
use crate::trace::TraceRecorder;
use crate::turn::TurnResult;

pub(super) struct CompletedTurn {
    pub(super) content: String,
    pub(super) reasoning_content: Option<String>,
    pub(super) model: String,
    pub(super) usage: pl_model::TokenUsage,
    pub(super) last_context_tokens: Option<u64>,
    pub(super) context_compactions: Vec<ContextCompactionSnapshot>,
    pub(super) session_message_count: usize,
    pub(super) completion: TurnCompletion,
}

pub(super) fn finish(
    recorder: &mut TraceRecorder,
    turn_id: &str,
    completed: CompletedTurn,
) -> TurnResult {
    recorder.ensure_assistant_text_item(turn_id, &completed.content);
    let outcome = TurnOutcome::completed(completed.completion);
    let completed_turn_item = recorder.terminal_turn_item(turn_id, &outcome);
    recorder.complete_item(completed_turn_item);
    recorder.broadcast(AgentEvent::Done);

    TurnResult {
        content: completed.content,
        reasoning_content: completed.reasoning_content,
        model: completed.model,
        usage: completed.usage,
        last_context_tokens: completed.last_context_tokens,
        context_compactions: completed.context_compactions,
        session_message_count: completed.session_message_count,
        outcome,
        trace_events: recorder.drain(),
    }
}
