//! Disposable full-text previews, independent of canonical completion accumulation.
use crate::completion::CompletionPresentationPartKind;
use crate::completion::stream::event::{ModelBlockContent, ModelBlockKind, ModelStreamEvent};
use pl_core::{
    chat::PresentationPart,
    context::{ContextContent, OpaquePayload},
    model::{ModelProgress, ModelProgressSender},
};
use pl_protocol::PureError;
use std::collections::BTreeMap;

pub(crate) struct ProgressProjection {
    sender: Option<ModelProgressSender>,
    order: Vec<String>,
    blocks: BTreeMap<String, (ModelBlockKind, String)>,
    presentation: Vec<OpaquePayload>,
    has_presentation: bool,
}
impl ProgressProjection {
    pub(crate) fn new(sender: Option<ModelProgressSender>) -> Self {
        if let Some(sender) = &sender {
            sender.publish(ModelProgress::default());
        }
        Self {
            sender,
            order: Vec::new(),
            blocks: BTreeMap::new(),
            presentation: Vec::new(),
            has_presentation: false,
        }
    }
    pub(crate) fn observe(&mut self, event: &ModelStreamEvent) -> Result<(), PureError> {
        if self.sender.is_none() {
            return Ok(());
        }
        match event {
            ModelStreamEvent::BlockOpened { id, kind, .. } => {
                if self.has_presentation {
                    return Ok(());
                }
                self.block(id, *kind);
            }
            ModelStreamEvent::BlockDelta {
                id, kind, delta, ..
            } => {
                if self.has_presentation {
                    return Ok(());
                }
                self.block(id, *kind).push_str(delta);
            }
            ModelStreamEvent::BlockClosed {
                id,
                kind,
                authoritative_content,
                ..
            } => {
                if self.has_presentation {
                    return Ok(());
                }
                if let Some(content) = authoritative_content {
                    *self.block(id, *kind) = match content {
                        ModelBlockContent::Text(text) => text.clone(),
                        ModelBlockContent::ReasoningSummary(parts) => parts.join("\n"),
                    };
                }
            }
            ModelStreamEvent::PresentationItem { item } => {
                let sender = self.sender.as_ref().expect("sender checked above");
                let reserve = |part| {
                    sender
                        .reserve_observed_item(&item.provider_item_id, part)
                        .map_err(|error| {
                            PureError::MemoryError(format!(
                                "presentation item reservation failed: {error}"
                            ))
                        })
                };
                if item.parts.is_empty() {
                    reserve(None)?;
                }
                for part in &item.parts {
                    let identity = match part.kind {
                        CompletionPresentationPartKind::OutputText => {
                            PresentationPart::OutputText(part.content_index)
                        }
                        CompletionPresentationPartKind::ReasoningText => {
                            PresentationPart::ReasoningText(part.content_index)
                        }
                        CompletionPresentationPartKind::SummaryText => {
                            PresentationPart::SummaryText(part.content_index)
                        }
                    };
                    reserve(Some(identity))?;
                }
                self.has_presentation = true;
                let encoded = serde_json::to_string(item)
                    .expect("presentation items contain only serializable text and indices");
                self.presentation.push(
                    OpaquePayload::new("pl.model.presentation-item", 1, encoded)
                        .expect("valid presentation payload format"),
                );
                if self.presentation.len() > 100 {
                    self.presentation.remove(0);
                }
            }
            ModelStreamEvent::ResponseStarted { .. }
            | ModelStreamEvent::ResponseModelObserved { .. }
            | ModelStreamEvent::ReasoningRawDelta { .. }
            | ModelStreamEvent::ToolInputStarted { .. }
            | ModelStreamEvent::ToolInputDelta { .. }
            | ModelStreamEvent::ToolInputCompleted { .. }
            | ModelStreamEvent::ToolCallReady { .. }
            | ModelStreamEvent::ToolCallCaller { .. }
            | ModelStreamEvent::ResponsesContextItem { .. }
            | ModelStreamEvent::WebSearchStarted { .. }
            | ModelStreamEvent::WebSearchCompleted { .. }
            | ModelStreamEvent::Usage(_)
            | ModelStreamEvent::Completed { .. }
            | ModelStreamEvent::Failed { .. } => return Ok(()),
        }
        let (mut text, mut reasoning) = (String::new(), String::new());
        if !self.has_presentation {
            for id in &self.order {
                if let Some((kind, content)) = self.blocks.get(id) {
                    match kind {
                        ModelBlockKind::Text { .. } => text.push_str(content),
                        ModelBlockKind::ReasoningSummary => reasoning.push_str(content),
                    }
                }
            }
        }
        if let Some(sender) = &self.sender {
            sender.publish(ModelProgress {
                content: if text.is_empty() {
                    Vec::new()
                } else {
                    vec![ContextContent::Text { text: text.into() }]
                },
                reasoning: (!reasoning.is_empty()).then(|| OpaquePayload::text(reasoning)),
                presentation: self.presentation.clone(),
            });
        }
        Ok(())
    }
    fn block(&mut self, id: &str, kind: ModelBlockKind) -> &mut String {
        if !self.blocks.contains_key(id) {
            self.order.push(id.to_owned());
        }
        &mut self
            .blocks
            .entry(id.to_owned())
            .or_insert((kind, String::new()))
            .1
    }
}
