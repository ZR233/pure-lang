use pl_protocol::{ErrorSeverity, TurnCompletion, TurnFailure, TurnFailureCategory, TurnOutcome};
use pl_trace::AgentEvent;

use crate::context_compaction::ContextCompactionSnapshot;
use crate::trace::TraceRecorder;
use crate::turn::TurnResult;

pub(super) struct CompletedTurn {
    pub(super) content: String,
    pub(super) reasoning_content: Option<String>,
    pub(super) model: String,
    pub(super) usage: pl_protocol::InferenceTokenUsage,
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
    let protocol_error =
        "turn completed while one or more trace items were still streaming".to_string();
    let open_items = recorder.fail_open_items(turn_id, &protocol_error);
    let publication_error = recorder
        .publication_error()
        .map(|error| format!("canonical trace publication failed: {error}"));
    let mut outcome = if let Some(message) = publication_error {
        recorder.broadcast(AgentEvent::Error {
            message: message.clone(),
            severity: ErrorSeverity::Fatal,
        });
        TurnOutcome::failed(TurnFailure::permanent(
            TurnFailureCategory::Protocol,
            message,
        ))
    } else if open_items.is_empty() {
        TurnOutcome::completed(completed.completion)
    } else {
        let message = format!("{protocol_error}: {}", open_items.join(", "));
        recorder.broadcast(AgentEvent::Error {
            message: message.clone(),
            severity: ErrorSeverity::Fatal,
        });
        TurnOutcome::failed(TurnFailure::permanent(
            TurnFailureCategory::Protocol,
            message,
        ))
    };
    recorder.finish_turn_item(turn_id, &outcome);
    if matches!(outcome, TurnOutcome::Completed(_))
        && let Some(error) = recorder.publication_error()
    {
        outcome = TurnOutcome::failed(TurnFailure::permanent(
            TurnFailureCategory::Protocol,
            format!("canonical trace publication failed: {error}"),
        ));
    }
    recorder.broadcast(AgentEvent::Done);

    TurnResult {
        billing: pl_protocol::TurnBillingRecord::new(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use pl_protocol::{TurnFailureCategory, TurnOutcome};
    use pl_trace::{TraceEventKind, TracePart, TraceTextChannel};

    #[test]
    fn nominal_completion_fails_when_a_trace_item_is_still_open() {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
        let mut recorder = TraceRecorder::new("session".to_string(), event_tx, 0);
        let turn_item = recorder.running_turn_item("turn");
        recorder.start_item(turn_item);
        recorder.start_item(TracePart::streaming_text(
            "turn",
            "turn:text",
            recorder.current_sequence(),
            TraceTextChannel::Final,
            1,
        ));

        let result = finish(
            &mut recorder,
            "turn",
            CompletedTurn {
                content: String::new(),
                reasoning_content: None,
                model: "test".to_string(),
                usage: pl_protocol::InferenceTokenUsage::default(),
                last_context_tokens: None,
                context_compactions: Vec::new(),
                session_message_count: 0,
                completion: TurnCompletion::Normal,
            },
        );

        assert!(matches!(
            result.outcome,
            TurnOutcome::Failed(ref failed)
                if failed.failure().category == TurnFailureCategory::Protocol
        ));
        let terminal_ids = result
            .trace_events
            .iter()
            .filter_map(|event| match &event.kind {
                TraceEventKind::TracePartCompleted { item }
                | TraceEventKind::TracePartFailed { item } => Some(item.item_id()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(terminal_ids, ["turn:text", "turn-turn"]);
    }

    #[test]
    fn closed_trace_owner_becomes_a_typed_protocol_failure() {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
        let (durable_tx, durable_rx) = tokio::sync::mpsc::unbounded_channel();
        drop(durable_rx);
        let mut recorder = TraceRecorder::streaming("session".to_string(), event_tx, 0, durable_tx);
        let turn_item = recorder.running_turn_item("turn");
        recorder.start_item(turn_item);

        let result = finish(
            &mut recorder,
            "turn",
            CompletedTurn {
                content: String::new(),
                reasoning_content: None,
                model: "test".to_string(),
                usage: pl_protocol::InferenceTokenUsage::default(),
                last_context_tokens: None,
                context_compactions: Vec::new(),
                session_message_count: 0,
                completion: TurnCompletion::Normal,
            },
        );

        assert!(matches!(
            result.outcome,
            TurnOutcome::Failed(ref failed)
                if failed.failure().category == TurnFailureCategory::Protocol
        ));
        assert!(result.trace_events.is_empty());
    }
}
