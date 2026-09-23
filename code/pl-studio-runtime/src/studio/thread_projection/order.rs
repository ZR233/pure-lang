//! One deterministic timeline order, derived from immutable fact admission rather than current rendering.
pub(super) fn compaction_id(id: &str) -> String {
    format!("compaction:{}:{id}", id.len())
}

pub(in crate::studio) fn turn_id(id: &str) -> String {
    format!("turn:{}:{id}", id.len())
}
pub(in crate::studio) fn message_id(id: &str) -> String {
    format!("message:{}:{id}", id.len())
}
pub(super) fn skill_id(id: &str) -> String {
    format!("skill:{}:{id}", id.len())
}

pub(super) fn completion_id(id: &str) -> String {
    format!("completion:{}:{id}", id.len())
}

pub(super) fn tool_id(id: &str) -> String {
    format!("tool:{}:{id}", id.len())
}

/// Durable receipt identity of one terminal interaction/permission record.
///
/// The writer records the receipt in the same transaction as the effect that produced it and the
/// host reads it back by this identity, so both sides derive it from the same function instead of
/// duplicating a format.
pub(in crate::studio) fn receipt_id(kind: &str, id: &str) -> String {
    format!("{kind}:{}:{id}", id.len())
}

pub(super) fn response_id(id: &str, kind: &str) -> String {
    format!("model:{}:{id}:{kind}", id.len())
}

pub(super) fn presentation_prefix(attempt_id: &str) -> String {
    pl_core::chat::presentation_prefix(attempt_id)
}

pub(in crate::studio) use pl_core::chat::PresentationPart;

/// Shared by receipt and preview projection. Output index and optional part ID can arrive after
/// streaming starts, so only the provider item's identity and the part's kind/index define a row.
pub(in crate::studio) fn presentation_id(
    attempt_id: &str,
    provider_item_id: &str,
    part: Option<PresentationPart>,
) -> String {
    pl_core::chat::presentation_item_id(attempt_id, provider_item_id, part)
}

pub(super) fn presentation_part(
    part: &pl_model::completion::CompletionPresentationPart,
) -> PresentationPart {
    use pl_model::completion::CompletionPresentationPartKind;
    match part.kind {
        CompletionPresentationPartKind::OutputText => {
            PresentationPart::OutputText(part.content_index)
        }
        CompletionPresentationPartKind::ReasoningText => {
            PresentationPart::ReasoningText(part.content_index)
        }
        CompletionPresentationPartKind::SummaryText => {
            PresentationPart::SummaryText(part.content_index)
        }
    }
}

pub(super) fn presentation_ids<'a>(
    attempt_id: &'a str,
    items: &'a [pl_model::completion::CompletionPresentationItem],
) -> impl Iterator<Item = String> + 'a {
    items.iter().flat_map(move |item| {
        item.parts
            .iter()
            .map(move |part| {
                presentation_id(
                    attempt_id,
                    &item.provider_item_id,
                    Some(presentation_part(part)),
                )
            })
            .chain(
                item.parts
                    .is_empty()
                    .then(|| presentation_id(attempt_id, &item.provider_item_id, None)),
            )
    })
}
