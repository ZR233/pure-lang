//! Final text emitted by the explicit completion tool, decoded from the saved producer receipt.
use super::order;
use pl_core::thread::{AttemptOutcome, ThreadSnapshot, ToolOutcome, journal::ThreadCommit};
use pl_protocol::{
    ThreadContentLifecycle, ThreadItem, ThreadItemState, ThreadTextChannel, ThreadTextItem,
};
use std::{collections::BTreeMap, sync::Arc};

pub(super) fn project_completions(
    thread_id: &str,
    snapshot: &ThreadSnapshot,
    journal: &[Arc<ThreadCommit>],
) -> Vec<ThreadItem> {
    let turns: BTreeMap<_, _> = snapshot
        .attempts
        .iter()
        .flat_map(|attempt| match &attempt.outcome {
            AttemptOutcome::Committed(output) => output
                .tool_calls
                .iter()
                .map(|call| (call.call_id.as_str(), attempt.turn_id.as_str()))
                .collect::<Vec<_>>(),
            AttemptOutcome::Running
            | AttemptOutcome::Interrupted
            | AttemptOutcome::Failed(_)
            | AttemptOutcome::Rejected { .. }
            | AttemptOutcome::Cancelled { .. } => Vec::new(),
        })
        .collect();
    let mut items = Vec::new();
    for commit in journal
        .iter()
        .filter(|commit| commit.sequence <= snapshot.commit_sequence)
    {
        for delivery in commit.deliveries.iter() {
            let Some(summary) = completion_summary(delivery) else {
                continue;
            };
            items.push(ThreadItem::new(
                order::completion_id(&delivery.call_id),
                thread_id.into(),
                turns
                    .get(delivery.call_id.as_str())
                    .copied()
                    .unwrap_or_default()
                    .into(),
                0,
                commit.sequence,
                commit.committed_at,
                commit.committed_at,
                ThreadItemState::Text(ThreadTextItem::new(
                    ThreadTextChannel::Final,
                    summary,
                    Vec::new(),
                    ThreadContentLifecycle::completed(commit.committed_at),
                )),
            ));
        }
    }
    items
}

fn completion_summary(delivery: &pl_core::thread::ToolDelivery) -> Option<String> {
    if delivery.tool_id != pl_tool::complete::TOOL_COMPLETE
        || !matches!(delivery.outcome, ToolOutcome::Succeeded)
        || delivery.output.control() != pl_core::tool::ToolControl::EndTurn
    {
        return None;
    }
    pl_tool::complete::saved_completion_summary(delivery.output.payload())
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
        let payload = OpaquePayload::new(
            "pl.tool.complete",
            1,
            r#"{"status":"completed","summary":"  original\r\n","evidence":[]}"#,
        )
        .unwrap();
        let mut delivery = ToolDelivery {
            target: Default::default(),
            call_id: "complete-call".into(),
            tool_id: "complete".into(),
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
        delivery.tool_id = "unrelated".into();
        assert_eq!(completion_summary(&delivery), None);
    }
}
