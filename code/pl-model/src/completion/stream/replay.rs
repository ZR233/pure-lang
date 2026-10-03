//! Bounded collection of the provider's replay view, before display filtering.

use super::super::AssistantReplay;
use pl_protocol::{PureError, Result};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const MAX_REPLAY_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone)]
pub enum ReplayUpdate {
    Item { index: Option<u32>, item: Value },
    Output { items: Vec<Value> },
    TextDelta(String),
}

impl ReplayUpdate {
    pub(crate) fn from_event(event: &crate::runtime::openai::sse::SseStreamEvent) -> Option<Self> {
        if let Some(choices) = &event.choices {
            return Some(Self::TextDelta(
                choices
                    .first()
                    .and_then(|choice| choice.delta.content.clone())
                    .unwrap_or_default(),
            ));
        }
        match event.kind.as_str() {
            "response.output_item.done" => event.item.clone().map(|item| Self::Item {
                index: event.output_index,
                item,
            }),
            "response.completed" => Some(Self::Output {
                items: event
                    .response
                    .as_ref()
                    .and_then(|response| response.get("output"))
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default(),
            }),
            _ => None,
        }
    }
}

#[derive(Default)]
pub(crate) struct ReplayCollector {
    items: Vec<(Option<u32>, Value)>,
    identities: BTreeMap<String, usize>,
    positions: BTreeSet<u32>,
    terminal: Option<Vec<Value>>,
    bytes: usize,
    chat: Option<String>,
}

impl ReplayCollector {
    pub(crate) fn apply(&mut self, update: ReplayUpdate) -> Result<()> {
        match update {
            ReplayUpdate::Item { index, item } => {
                let size = serde_json::to_vec(&item)?.len();
                if let Some(id) = item.get("id").and_then(Value::as_str)
                    && let Some(&position) = self.identities.get(id)
                {
                    let (old_index, old) = &self.items[position];
                    if old_index.is_some() && index.is_some() && *old_index != index || old != &item
                    {
                        return Err(PureError::Protocol(
                            "conflicting completed assistant output item".into(),
                        ));
                    }
                    return Ok(());
                }
                if let Some(index) = index
                    && !self.positions.insert(index)
                {
                    return Err(PureError::Protocol(
                        "duplicate assistant output index".into(),
                    ));
                }
                if let Some(id) = item.get("id").and_then(Value::as_str) {
                    self.identities.insert(id.to_owned(), self.items.len());
                }
                self.bytes = bounded(self.bytes.checked_add(size))?;
                self.items.push((index, item));
            }
            ReplayUpdate::Output { items } => {
                bounded(Some(serde_json::to_vec(&items)?.len()))?;
                self.terminal = Some(items);
            }
            ReplayUpdate::TextDelta(delta) => {
                let text = self.chat.get_or_insert_with(String::new);
                bounded(text.len().checked_add(delta.len()))?;
                text.push_str(&delta);
            }
        }
        Ok(())
    }

    pub(crate) fn finish(&mut self) -> Result<Option<AssistantReplay>> {
        if let Some(content) = self.chat.take() {
            return Ok(Some(AssistantReplay::Chat { content }));
        }
        let terminal = self.terminal.take();
        // Some compatible endpoints send an abbreviated terminal envelope without item identities.
        // Completed items remain authoritative there; an identity-bearing output is checked whole.
        if let Some(items) = terminal.as_ref()
            && !items.is_empty()
            && items
                .iter()
                .all(|item| item.get("id").and_then(Value::as_str).is_some())
        {
            for (index, done) in &self.items {
                let candidate = index
                    .and_then(|index| items.get(index as usize))
                    .or_else(|| items.iter().find(|item| item.get("id") == done.get("id")));
                if candidate != Some(done) {
                    return Err(PureError::Protocol(
                        "terminal assistant output differs from completed items".into(),
                    ));
                }
            }
            self.items.clear();
            return Ok(Some(AssistantReplay::Responses {
                output: terminal.unwrap_or_default(),
            }));
        }
        if !self.items.is_empty() {
            self.items
                .sort_by_key(|(index, _)| index.unwrap_or(u32::MAX));
            for (expected, (index, _)) in self.items.iter().enumerate() {
                if index.is_some_and(|index| index as usize != expected) {
                    return Err(PureError::Protocol(
                        "incomplete assistant output indices".into(),
                    ));
                }
            }
            return Ok(Some(AssistantReplay::Responses {
                output: std::mem::take(&mut self.items)
                    .into_iter()
                    .map(|(_, item)| item)
                    .collect(),
            }));
        }
        Ok(terminal.map(|output| AssistantReplay::Responses { output }))
    }
}

fn bounded(size: Option<usize>) -> Result<usize> {
    size.filter(|size| *size <= MAX_REPLAY_BYTES)
        .ok_or_else(|| PureError::MemoryError("provider replay output exceeds 16 MiB".into()))
}
