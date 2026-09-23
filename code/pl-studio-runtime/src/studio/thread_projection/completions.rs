//! Final text emitted by the explicit completion tool, decoded from the saved producer receipt.
use super::order;
use pl_core::thread::{AttemptOutcome, ThreadEffectBatch, ThreadSnapshot, ToolOutcome};
use pl_protocol::{
    ThreadContentLifecycle, ThreadItem, ThreadItemState, ThreadTextChannel, ThreadTextItem,
};
use std::{collections::BTreeMap, sync::Arc};

pub(super) fn project_completions(
    thread_id: &str,
    snapshot: &ThreadSnapshot,
    journal: &[Arc<ThreadEffectBatch>],
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
            if let Some(item) = project_completion(
                thread_id,
                turns
                    .get(delivery.call_id.as_str())
                    .copied()
                    .unwrap_or_default(),
                delivery,
                commit.sequence,
                commit.committed_at,
            ) {
                items.push(item);
            }
        }
    }
    items
}

pub(super) fn project_completion(
    thread_id: &str,
    turn_id: &str,
    delivery: &pl_core::thread::ToolDelivery,
    revision: u64,
    at: i64,
) -> Option<ThreadItem> {
    let summary = completion_summary(delivery)?;
    Some(ThreadItem::new(
        order::completion_id(&delivery.call_id),
        thread_id.into(),
        turn_id.into(),
        0,
        revision,
        at,
        at,
        ThreadItemState::Text(ThreadTextItem::new(
            ThreadTextChannel::Final,
            summary,
            Vec::new(),
            ThreadContentLifecycle::completed(at),
        )),
    ))
}

fn completion_summary(delivery: &pl_core::thread::ToolDelivery) -> Option<String> {
    if !matches!(delivery.outcome, ToolOutcome::Succeeded)
        || delivery.output.control() != pl_core::tool::ToolControl::EndTurn
    {
        return None;
    }
    pl_tool::finish_turn::saved_message(delivery.output.payload())
        .ok()
        .flatten()
}
