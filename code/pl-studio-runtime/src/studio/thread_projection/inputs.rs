//! Accepted input items retain stable product identity across queueing, steering and retries.
use super::{ProjectionError, content::input_content};
use pl_core::thread::{ThreadSnapshot, input::InputState};
use pl_protocol::{
    MessagePresentation, ThreadContentLifecycle, ThreadItem, ThreadItemState, ThreadTextChannel,
    ThreadTextItem,
};

#[allow(clippy::too_many_arguments)]
pub(super) fn project_input(
    thread_id: &str,
    snapshot: &ThreadSnapshot,
    input: &pl_core::thread::input::InputRecord,
    ordinal: u64,
    created_at: i64,
    revision: u64,
    updated_at: i64,
) -> Result<Option<ThreadItem>, ProjectionError> {
    let content = input_content(input);
    if content
        .as_ref()
        .is_ok_and(|content| content.presentation == MessagePresentation::Hidden)
    {
        return Ok(None);
    }
    let turn_id = match &input.state {
        InputState::Consumed { turn_id, .. } => turn_id.clone(),
        InputState::Pending | InputState::Discarded => snapshot
            .turns
            .iter()
            .rev()
            .find(|turn| turn.input_id.as_deref() == Some(input.input.id.as_str()))
            .map(|turn| turn.turn_id.clone())
            .unwrap_or_default(),
    };
    let lifecycle = match input.state {
        InputState::Pending | InputState::Consumed { .. } => {
            ThreadContentLifecycle::completed(created_at)
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
            payloads: vec![super::raw_payload(&input.input.payload)],
            notice: error.to_string(),
            recorded_at: updated_at,
        }),
    };
    Ok(Some(ThreadItem::new(
        input.input.id.clone(),
        thread_id.into(),
        turn_id,
        ordinal,
        revision,
        created_at,
        updated_at,
        state,
    )))
}
