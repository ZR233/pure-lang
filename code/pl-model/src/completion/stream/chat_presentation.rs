//! Completed Chat display segments, derived with the same decoder as live text.
use super::event::{ModelBlockKind, ModelStreamEvent};
use super::tagged_output::TaggedVisibleOutputAdapter;
use crate::completion::{
    CompletionPresentationItem, CompletionPresentationItemKind, CompletionPresentationPart,
    CompletionPresentationPartKind,
};
use pl_protocol::trace::TraceTextChannel;
use pl_protocol::{PureError, Result};

pub(super) fn completed_chat_presentation(
    content: &str,
    reasoning: Option<&str>,
) -> Result<Vec<CompletionPresentationItem>> {
    let mut items = Vec::new();
    if let Some(reasoning) = reasoning.filter(|text| !text.is_empty()) {
        items.push(item(
            "chat-reasoning".into(),
            CompletionPresentationItemKind::Reasoning,
            CompletionPresentationPartKind::ReasoningText,
            reasoning.to_owned(),
        ));
    }
    let mut decoder = TaggedVisibleOutputAdapter::new();
    let events = decoder.adapt(ModelStreamEvent::text_delta(
        "chat-content".into(),
        TraceTextChannel::Final,
        content.to_owned(),
        None,
    ));
    let terminal = decoder.adapt(ModelStreamEvent::Completed { response_id: None });
    for event in events.into_iter().chain(terminal) {
        match event {
            ModelStreamEvent::BlockOpened {
                id,
                kind: ModelBlockKind::Text { channel },
                ..
            } => items.push(item(
                id,
                CompletionPresentationItemKind::Text(channel),
                CompletionPresentationPartKind::OutputText,
                String::new(),
            )),
            ModelStreamEvent::BlockDelta {
                id,
                kind: ModelBlockKind::Text { channel },
                delta,
                ..
            } => {
                let current = items
                    .last_mut()
                    .filter(|item| {
                        item.provider_item_id == id
                            && item.kind == CompletionPresentationItemKind::Text(channel)
                    })
                    .ok_or_else(|| {
                        PureError::Protocol("Chat display delta has no matching segment".into())
                    })?;
                current.parts[0].text.push_str(&delta);
            }
            _ => {}
        }
    }
    Ok(items)
}

fn item(
    id: String,
    kind: CompletionPresentationItemKind,
    part: CompletionPresentationPartKind,
    text: String,
) -> CompletionPresentationItem {
    CompletionPresentationItem {
        provider_item_id: id,
        output_index: None,
        kind,
        parts: vec![CompletionPresentationPart {
            content_index: 0,
            provider_part_id: None,
            kind: part,
            text,
        }],
    }
}
