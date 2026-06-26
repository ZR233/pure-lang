use std::collections::{BTreeMap, HashMap, btree_map::Entry};

use super::event::{
    ModelBlockContent, ModelBlockKind, ModelStreamEvent, ToolInputDeltaPayload,
    ToolInputPayloadKind,
};

pub(crate) struct StreamLifecycle {
    open_blocks: BTreeMap<String, OpenBlock>,
    open_tools: HashMap<String, OpenToolInput>,
}

#[derive(Debug, Clone)]
struct OpenBlock {
    id: String,
    kind: ModelBlockKind,
}

struct OpenToolInput {
    stream_id: Option<String>,
    item_id: String,
    call_id: Option<String>,
    name: Option<String>,
}

impl StreamLifecycle {
    pub(crate) fn new() -> Self {
        Self {
            open_blocks: BTreeMap::new(),
            open_tools: HashMap::new(),
        }
    }

    pub(crate) fn normalize(&mut self, event: ModelStreamEvent) -> Vec<ModelStreamEvent> {
        match event {
            ModelStreamEvent::BlockOpened {
                id,
                kind,
                provider_metadata,
            } => {
                self.open_blocks.insert(
                    block_key(kind, &id),
                    OpenBlock {
                        id: id.clone(),
                        kind,
                    },
                );
                vec![ModelStreamEvent::BlockOpened {
                    id,
                    kind,
                    provider_metadata,
                }]
            }
            ModelStreamEvent::BlockDelta {
                id,
                kind,
                field,
                delta,
                section_index,
            } => {
                let key = block_key(kind, &id);
                match self.open_blocks.entry(key) {
                    Entry::Occupied(_) => vec![ModelStreamEvent::BlockDelta {
                        id,
                        kind,
                        field,
                        delta,
                        section_index,
                    }],
                    Entry::Vacant(entry) => {
                        entry.insert(OpenBlock {
                            id: id.clone(),
                            kind,
                        });
                        vec![
                            ModelStreamEvent::BlockOpened {
                                id: id.clone(),
                                kind,
                                provider_metadata: None,
                            },
                            ModelStreamEvent::BlockDelta {
                                id,
                                kind,
                                field,
                                delta,
                                section_index,
                            },
                        ]
                    }
                }
            }
            ModelStreamEvent::BlockClosed {
                id,
                kind,
                authoritative_content,
                provider_metadata,
            } => {
                let key = block_key(kind, &id);
                if self.open_blocks.remove(&key).is_some() {
                    vec![ModelStreamEvent::BlockClosed {
                        id,
                        kind,
                        authoritative_content,
                        provider_metadata,
                    }]
                } else if authoritative_content
                    .as_ref()
                    .is_some_and(block_content_is_non_empty)
                {
                    vec![
                        ModelStreamEvent::BlockOpened {
                            id: id.clone(),
                            kind,
                            provider_metadata: None,
                        },
                        ModelStreamEvent::BlockClosed {
                            id,
                            kind,
                            authoritative_content,
                            provider_metadata,
                        },
                    ]
                } else {
                    Vec::new()
                }
            }
            ModelStreamEvent::ReasoningRawDelta {
                id,
                content_index,
                delta,
            } => vec![ModelStreamEvent::ReasoningRawDelta {
                id,
                content_index,
                delta,
            }],
            ModelStreamEvent::ToolInputStarted {
                stream_id,
                item_id,
                call_id,
                name,
                payload_kind,
            } => {
                let mut events = self.close_open_content_blocks();
                let key = self.resolve_tool_key(stream_id.as_ref(), call_id.as_ref(), &item_id);
                self.upsert_tool_input(
                    &key,
                    stream_id.as_ref(),
                    &item_id,
                    call_id.as_ref(),
                    name.as_ref(),
                );
                events.push(ModelStreamEvent::ToolInputStarted {
                    stream_id,
                    item_id,
                    call_id,
                    name,
                    payload_kind,
                });
                events
            }
            ModelStreamEvent::ToolInputDelta {
                stream_id,
                item_id,
                call_id,
                name,
                payload_delta,
            } => {
                let key = self.resolve_tool_key(stream_id.as_ref(), call_id.as_ref(), &item_id);
                let is_new = !self.open_tools.contains_key(&key);
                self.upsert_tool_input(
                    &key,
                    stream_id.as_ref(),
                    &item_id,
                    call_id.as_ref(),
                    name.as_ref(),
                );
                if is_new {
                    let mut events = self.close_open_content_blocks();
                    events.push(ModelStreamEvent::ToolInputStarted {
                        stream_id: stream_id.clone(),
                        item_id: item_id.clone(),
                        call_id: call_id.clone(),
                        name: name.clone(),
                        payload_kind: payload_delta.kind(),
                    });
                    events.push(ModelStreamEvent::ToolInputDelta {
                        stream_id,
                        item_id,
                        call_id,
                        name,
                        payload_delta,
                    });
                    events
                } else {
                    vec![ModelStreamEvent::ToolInputDelta {
                        stream_id,
                        item_id,
                        call_id,
                        name,
                        payload_delta,
                    }]
                }
            }
            ModelStreamEvent::ToolInputCompleted {
                stream_id,
                item_id,
                call_id,
                name,
                payload,
            } => {
                let key = self.resolve_tool_key(stream_id.as_ref(), call_id.as_ref(), &item_id);
                self.open_tools.remove(&key);
                vec![ModelStreamEvent::ToolInputCompleted {
                    stream_id,
                    item_id,
                    call_id,
                    name,
                    payload,
                }]
            }
            ModelStreamEvent::Completed { response_id } => {
                let mut events = self.close_open_blocks();
                events.push(ModelStreamEvent::Completed { response_id });
                events
            }
            ModelStreamEvent::StepStarted { response_id } => {
                let mut events = self.close_open_content_blocks();
                events.push(ModelStreamEvent::StepStarted { response_id });
                events
            }
            ModelStreamEvent::ToolCallReady {
                stream_id,
                item_id,
                call_id,
                name,
                payload,
            } => {
                let mut events = self.close_open_content_blocks();
                let key = self.resolve_tool_key(stream_id.as_ref(), call_id.as_ref(), &item_id);
                self.open_tools.remove(&key);
                events.push(ModelStreamEvent::ToolCallReady {
                    stream_id,
                    item_id,
                    call_id,
                    name,
                    payload,
                });
                events
            }
            ModelStreamEvent::Usage(usage) => vec![ModelStreamEvent::Usage(usage)],
            ModelStreamEvent::Failed { code, message } => {
                let mut events = self.close_open_blocks();
                events.push(ModelStreamEvent::Failed { code, message });
                events
            }
        }
    }

    fn close_open_blocks(&mut self) -> Vec<ModelStreamEvent> {
        let mut events = self.close_open_content_blocks();
        for (_, tool) in std::mem::take(&mut self.open_tools) {
            events.push(ModelStreamEvent::ToolInputCompleted {
                stream_id: tool.stream_id,
                item_id: tool.item_id,
                call_id: tool.call_id,
                name: tool.name,
                payload: None,
            });
        }
        events
    }

    fn close_open_content_blocks(&mut self) -> Vec<ModelStreamEvent> {
        std::mem::take(&mut self.open_blocks)
            .into_values()
            .map(|block| ModelStreamEvent::BlockClosed {
                id: block.id,
                kind: block.kind,
                authoritative_content: None,
                provider_metadata: None,
            })
            .collect()
    }

    fn upsert_tool_input(
        &mut self,
        key: &str,
        stream_id: Option<&String>,
        item_id: &str,
        call_id: Option<&String>,
        name: Option<&String>,
    ) {
        let entry = self
            .open_tools
            .entry(key.to_string())
            .or_insert_with(|| OpenToolInput {
                stream_id: stream_id.cloned(),
                item_id: item_id.to_string(),
                call_id: call_id.cloned(),
                name: None,
            });
        if entry.stream_id.as_ref().is_none_or(String::is_empty)
            && let Some(stream_id) = stream_id.filter(|value| !value.is_empty())
        {
            entry.stream_id = Some(stream_id.clone());
        }
        if entry.item_id.is_empty() && !item_id.is_empty() {
            entry.item_id = item_id.to_string();
        }
        if entry.call_id.as_ref().is_none_or(String::is_empty)
            && let Some(call_id) = call_id.filter(|value| !value.is_empty())
        {
            entry.call_id = Some(call_id.clone());
        }
        if entry.name.as_ref().is_none_or(String::is_empty)
            && let Some(name) = name.filter(|value| !value.is_empty())
        {
            entry.name = Some(name.clone());
        }
    }

    fn resolve_tool_key(
        &self,
        stream_id: Option<&String>,
        call_id: Option<&String>,
        item_id: &str,
    ) -> String {
        let key = tool_key(stream_id, call_id, item_id);
        if self.open_tools.contains_key(&key) {
            return key;
        }
        self.open_tools
            .iter()
            .find_map(|(key, tool)| tool.matches_identity(call_id, item_id).then(|| key.clone()))
            .unwrap_or(key)
    }
}

impl OpenToolInput {
    fn matches_identity(&self, call_id: Option<&String>, item_id: &str) -> bool {
        let call_id_matches = call_id
            .filter(|value| !value.is_empty())
            .zip(self.call_id.as_ref())
            .is_some_and(|(left, right)| left == right);
        let item_id_matches = !item_id.is_empty() && self.item_id == item_id;
        call_id_matches || item_id_matches
    }
}

fn block_key(kind: ModelBlockKind, id: &str) -> String {
    format!("{}:{id}", block_kind_label(kind))
}

fn block_kind_label(kind: ModelBlockKind) -> &'static str {
    match kind {
        ModelBlockKind::Text { channel } => channel.as_str(),
        ModelBlockKind::ReasoningSummary => "reasoning.summary",
        ModelBlockKind::Plan => "plan",
    }
}

fn block_content_is_non_empty(content: &ModelBlockContent) -> bool {
    match content {
        ModelBlockContent::Text(text) | ModelBlockContent::Plan(text) => !text.is_empty(),
        ModelBlockContent::ReasoningSummary(summary) => summary.iter().any(|item| !item.is_empty()),
    }
}

fn tool_key(stream_id: Option<&String>, call_id: Option<&String>, item_id: &str) -> String {
    stream_id
        .filter(|value| !value.is_empty())
        .cloned()
        .or_else(|| call_id.filter(|value| !value.is_empty()).cloned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| item_id.to_string())
}

pub(crate) fn tool_start_payload(kind: ToolInputPayloadKind) -> ToolInputDeltaPayload {
    kind.empty_payload()
}

#[cfg(test)]
mod tests {
    use pl_trace::TraceTextChannel;
    use pretty_assertions::assert_eq;

    use crate::stream::event::ModelBlockField;

    use super::*;

    #[test]
    fn authoritative_completion_without_delta_still_starts_and_completes_text() {
        let mut lifecycle = StreamLifecycle::new();

        let events = lifecycle.normalize(ModelStreamEvent::text_completed(
            "msg_progress".to_string(),
            TraceTextChannel::Commentary,
            Some("已完成检查".to_string()),
        ));

        match events.as_slice() {
            [
                ModelStreamEvent::BlockOpened { id, kind, .. },
                ModelStreamEvent::BlockClosed {
                    id: completed_id,
                    kind: completed_kind,
                    authoritative_content,
                    ..
                },
            ] => {
                assert_eq!(id, "msg_progress");
                assert_eq!(
                    *kind,
                    ModelBlockKind::Text {
                        channel: TraceTextChannel::Commentary
                    }
                );
                assert_eq!(completed_id, "msg_progress");
                assert_eq!(*completed_kind, *kind);
                assert!(matches!(
                    authoritative_content,
                    Some(ModelBlockContent::Text(text)) if text == "已完成检查"
                ));
            }
            other => panic!("unexpected events: {other:?}"),
        }
    }

    #[test]
    fn delta_without_start_opens_block_before_delta() {
        let mut lifecycle = StreamLifecycle::new();

        let events = lifecycle.normalize(ModelStreamEvent::reasoning_summary_delta(
            "thinking".to_string(),
            0,
            "summary".to_string(),
        ));

        assert!(matches!(
            events.as_slice(),
            [
                ModelStreamEvent::BlockOpened {
                    id,
                    kind: ModelBlockKind::ReasoningSummary,
                    ..
                },
                ModelStreamEvent::BlockDelta {
                    id: delta_id,
                    kind: ModelBlockKind::ReasoningSummary,
                    field: ModelBlockField::ReasoningSummary,
                    delta,
                    section_index: Some(0),
                },
            ] if id == "thinking" && delta_id == id && delta == "summary"
        ));
    }
}
