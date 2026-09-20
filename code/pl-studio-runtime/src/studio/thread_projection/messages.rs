//! Parent dialogue is projected from durable inbox admission and consumption, never model context.
use pl_core::thread::inbox::ThreadMessage;
use pl_protocol::{ThreadItem, ThreadItemState, ThreadRawItem};

/// Builds one parent-agent conversation item.
///
/// Message identity and admission time are fixed by the inbox record; `turn_id`, `revision` and
/// `updated_at` carry the later consumption watermark once the message is delivered to a Turn.
pub(super) fn message_item(
    thread_id: &str,
    message: &ThreadMessage,
    turn_id: &str,
    revision: u64,
    created_at: i64,
    updated_at: i64,
) -> ThreadItem {
    let payload = &message.payload;
    let state = if payload.format() == "text/plain" && payload.version() == 1 {
        ThreadItem::completed_parent_agent_message(
            super::order::message_id(&message.id),
            thread_id.into(),
            String::new(),
            payload.content().into(),
            Vec::new(),
            created_at,
        )
        .state()
        .clone()
    } else {
        ThreadItemState::Raw(ThreadRawItem {
            payloads: vec![super::raw_payload(payload)],
            notice: "Unsupported saved parent message encoding".into(),
            recorded_at: created_at,
        })
    };
    ThreadItem::new(
        super::order::message_id(&message.id),
        thread_id.into(),
        turn_id.into(),
        0,
        revision,
        created_at,
        updated_at,
        state,
    )
}
