//! Disposable full-text previews, independent of canonical completion accumulation.
use crate::completion::stream::event::{ModelBlockContent, ModelBlockKind, ModelStreamEvent};
use pl_core::{
    context::{ContextContent, OpaquePayload},
    model::{ModelProgress, ModelProgressSender},
};
use std::collections::BTreeMap;

pub(crate) struct ProgressProjection {
    sender: Option<ModelProgressSender>,
    order: Vec<String>,
    blocks: BTreeMap<String, (ModelBlockKind, String)>,
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
        }
    }
    pub(crate) fn observe(&mut self, event: &ModelStreamEvent) {
        if self.sender.is_none() {
            return;
        }
        match event {
            ModelStreamEvent::BlockOpened { id, kind, .. } => {
                self.block(id, *kind);
            }
            ModelStreamEvent::BlockDelta {
                id, kind, delta, ..
            } => {
                self.block(id, *kind).push_str(delta);
            }
            ModelStreamEvent::BlockClosed {
                id,
                kind,
                authoritative_content,
                ..
            } => {
                if let Some(content) = authoritative_content {
                    *self.block(id, *kind) = match content {
                        ModelBlockContent::Text(text) => text.clone(),
                        ModelBlockContent::ReasoningSummary(parts) => parts.join("\n"),
                    };
                }
            }
            ModelStreamEvent::ResponseStarted { .. }
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
            | ModelStreamEvent::Failed { .. } => return,
        }
        let mut text = String::new();
        let mut reasoning = String::new();
        for id in &self.order {
            if let Some((kind, content)) = self.blocks.get(id) {
                match kind {
                    ModelBlockKind::Text { .. } => text.push_str(content),
                    ModelBlockKind::ReasoningSummary => reasoning.push_str(content),
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
            });
        }
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
