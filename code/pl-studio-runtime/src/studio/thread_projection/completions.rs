//! Final text emitted by the explicit completion tool, decoded from the saved producer receipt.
use pl_core::thread::{ToolDelivery, ToolOutcome};
use pl_protocol::{
    ThreadContentLifecycle, ThreadItem, ThreadItemState, ThreadTextChannel, ThreadTextItem,
};

/// Builds one completion item from a saved delivery, or nothing when the delivery is not a
/// successful end-Turn completion.
pub(super) fn completion_item(
    thread_id: &str,
    delivery: &ToolDelivery,
    turn_id: &str,
    sequence: u64,
    committed_at: i64,
) -> Option<ThreadItem> {
    let summary = completion_summary(delivery)?;
    Some(ThreadItem::new(
        super::order::completion_id(&delivery.call_id),
        thread_id.into(),
        turn_id.into(),
        0,
        sequence,
        committed_at,
        committed_at,
        ThreadItemState::Text(ThreadTextItem::new(
            ThreadTextChannel::Final,
            summary,
            Vec::new(),
            ThreadContentLifecycle::completed(committed_at),
        )),
    ))
}

fn completion_summary(delivery: &ToolDelivery) -> Option<String> {
    if !matches!(delivery.outcome, ToolOutcome::Succeeded)
        || delivery.output.control() != pl_core::tool::ToolControl::EndTurn
    {
        return None;
    }
    pl_tool::finish_turn::saved_message(delivery.output.payload())
        .ok()
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_core::{context::OpaquePayload, thread::ToolDelivery, tool::ToolOutput};
    use pretty_assertions::assert_eq;

    #[test]
    fn completion_display_requires_success_and_trusted_end_turn_control() {
        let payload =
            OpaquePayload::new("pl.tool.finish-turn", 1, r#"{"message":"  original\r\n"}"#)
                .unwrap();
        let mut delivery = ToolDelivery {
            target: Default::default(),
            call_id: "complete-call".into(),
            tool_id: "finish_turn".into(),
            output: ToolOutput::new(payload, Vec::new()),
            delivered_context: Vec::new(),
            outcome: ToolOutcome::Succeeded,
        };
        assert_eq!(
            completion_summary(&delivery),
            None,
            "payload alone cannot claim completion"
        );
        delivery.output = delivery.output.ending_turn();
        assert_eq!(
            completion_summary(&delivery).as_deref(),
            Some("  original\r\n")
        );
        delivery.outcome = ToolOutcome::Cancelled;
        assert_eq!(completion_summary(&delivery), None);
        delivery.outcome = ToolOutcome::Succeeded;
    }
}
