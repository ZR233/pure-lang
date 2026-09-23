//! Final text emitted by the explicit completion tool, decoded from the saved producer receipt.
use super::order;
use pl_core::thread::ToolOutcome;
use pl_protocol::{
    ThreadContentLifecycle, ThreadItem, ThreadItemState, ThreadTextChannel, ThreadTextItem,
};

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
