//! Accepted input items retain stable product identity across queueing, steering and retries.
use super::content::input_content;
use pl_core::thread::input::{InputRecord, InputState};
use pl_protocol::{
    MessagePresentation, ThreadContentLifecycle, ThreadItem, ThreadItemState, ThreadTextChannel,
    ThreadTextItem,
};

/// Builds one accepted input's display item from its admitted facts.
///
/// Hidden presentations keep their reserved ordinal but produce no item; an unknown saved payload
/// stays an explicit raw record instead of degrading to empty text. `accepted` and `updated` are the
/// admitting and last-touching `(sequence, committed_at)` pairs, and `turn_id` is resolved by the
/// caller from the consumption state or the owning Turn.
pub(super) fn input_item(
    thread_id: &str,
    record: &InputRecord,
    accepted: (u64, i64),
    updated: (u64, i64),
    turn_id: &str,
) -> Option<ThreadItem> {
    let content = input_content(record);
    if content
        .as_ref()
        .is_ok_and(|content| content.presentation == MessagePresentation::Hidden)
    {
        return None;
    }
    let (_, accepted_at) = accepted;
    let (updated_sequence, updated_at) = updated;
    let lifecycle = match record.state {
        InputState::Pending | InputState::Consumed { .. } => {
            ThreadContentLifecycle::completed(accepted_at)
        }
        InputState::Discarded => ThreadContentLifecycle::cancelled(
            updated_at,
            "Input discarded before model admission.".into(),
        ),
    };
    let state = match content {
        Ok(content) => ThreadItemState::Text(ThreadTextItem::new(
            ThreadTextChannel::User,
            content.text,
            content.attachments,
            lifecycle,
        )),
        Err(error) => ThreadItemState::Raw(pl_protocol::ThreadRawItem {
            payloads: vec![super::raw_payload(&record.input.payload)],
            notice: error.to_string(),
            recorded_at: updated_at,
        }),
    };
    Some(ThreadItem::new(
        record.input.id.clone(),
        thread_id.into(),
        turn_id.into(),
        0,
        updated_sequence,
        accepted_at,
        updated_at,
        state,
    ))
}
