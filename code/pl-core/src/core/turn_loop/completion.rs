use pl_protocol::TokenUsageSnapshot;
use pl_trace::{AgentEvent, TracePartStatus};

use crate::context_compaction::ContextCompactionSnapshot;
use crate::trace::TraceRecorder;
use crate::turn::{TurnResult, TurnResultStatus};

pub(super) struct CompletedTurn {
    pub(super) content: String,
    pub(super) reasoning_content: Option<String>,
    pub(super) model: String,
    pub(super) usage: pl_model::TokenUsage,
    pub(super) last_context_tokens: Option<u64>,
    pub(super) context_compactions: Vec<ContextCompactionSnapshot>,
    pub(super) session_message_count: usize,
    pub(super) inference_count: u64,
    pub(super) ended_for_interaction: bool,
}

pub(super) fn finish(
    recorder: &mut TraceRecorder,
    turn_id: &str,
    completed: CompletedTurn,
) -> TurnResult {
    recorder.ensure_assistant_text_item(turn_id, &completed.content);
    let mut completed_turn_item = recorder.turn_item(turn_id, TracePartStatus::Completed);
    completed_turn_item.content = completed.content.clone();
    completed_turn_item.usage = Some(TokenUsageSnapshot {
        prompt_tokens: completed.usage.prompt_tokens,
        completion_tokens: completed.usage.completion_tokens,
        cached_prompt_tokens: completed.usage.cached_prompt_tokens,
        cache_write_tokens: completed.usage.cache_write_tokens,
        cache_miss_tokens: completed.usage.prompt_tokens.saturating_sub(
            completed
                .usage
                .cached_prompt_tokens
                .min(completed.usage.prompt_tokens),
        ),
        reasoning_tokens: completed.usage.reasoning_tokens,
        inference_count: completed.inference_count,
        total_tokens: completed.usage.total_tokens,
    });
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
        status: TurnResultStatus::Completed,
        ended_for_interaction: completed.ended_for_interaction,
        abort_reason: None,
        error: None,
        failure: None,
        budget_limit_kind: None,
        budget_usage: None,
        rollover_compacted: false,
        rollover_compaction_error: None,
        trace_events: recorder.drain(),
    }
}
