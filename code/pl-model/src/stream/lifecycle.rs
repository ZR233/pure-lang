use std::collections::{BTreeSet, HashMap};

use pl_trace::TraceTextChannel;

use super::event::{ModelStreamEvent, ToolInputDeltaPayload, ToolInputPayloadKind};

pub(crate) struct StreamLifecycle {
    open_text: BTreeSet<String>,
    open_reasoning: BTreeSet<String>,
    open_plan: BTreeSet<String>,
    open_tools: HashMap<String, OpenToolInput>,
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
            open_text: BTreeSet::new(),
            open_reasoning: BTreeSet::new(),
            open_plan: BTreeSet::new(),
            open_tools: HashMap::new(),
        }
    }

    pub(crate) fn normalize(&mut self, event: ModelStreamEvent) -> Vec<ModelStreamEvent> {
        match event {
            ModelStreamEvent::TextStarted { id, channel } => {
                self.open_text.insert(block_key(channel.as_str(), &id));
                vec![ModelStreamEvent::TextStarted { id, channel }]
            }
            ModelStreamEvent::TextDelta { id, channel, delta } => {
                let key = block_key(channel.as_str(), &id);
                if self.open_text.insert(key) {
                    vec![
                        ModelStreamEvent::TextStarted {
                            id: id.clone(),
                            channel,
                        },
                        ModelStreamEvent::TextDelta { id, channel, delta },
                    ]
                } else {
                    vec![ModelStreamEvent::TextDelta { id, channel, delta }]
                }
            }
            ModelStreamEvent::TextCompleted {
                id,
                channel,
                authoritative_text,
            } => {
                self.open_text.remove(&block_key(channel.as_str(), &id));
                vec![ModelStreamEvent::TextCompleted {
                    id,
                    channel,
                    authoritative_text,
                }]
            }
            ModelStreamEvent::ReasoningStarted {
                id,
                provider_metadata,
            } => {
                self.open_reasoning.insert(id.clone());
                vec![ModelStreamEvent::ReasoningStarted {
                    id,
                    provider_metadata,
                }]
            }
            ModelStreamEvent::ReasoningDelta {
                id,
                chunk_index,
                delta,
            } => {
                if self.open_reasoning.insert(id.clone()) {
                    vec![
                        ModelStreamEvent::ReasoningStarted {
                            id: id.clone(),
                            provider_metadata: None,
                        },
                        ModelStreamEvent::ReasoningDelta {
                            id,
                            chunk_index,
                            delta,
                        },
                    ]
                } else {
                    vec![ModelStreamEvent::ReasoningDelta {
                        id,
                        chunk_index,
                        delta,
                    }]
                }
            }
            ModelStreamEvent::ReasoningCompleted {
                id,
                provider_metadata,
            } => {
                self.open_reasoning.remove(&id);
                vec![ModelStreamEvent::ReasoningCompleted {
                    id,
                    provider_metadata,
                }]
            }
            ModelStreamEvent::PlanStarted { id } => {
                self.open_plan.insert(id.clone());
                vec![ModelStreamEvent::PlanStarted { id }]
            }
            ModelStreamEvent::PlanDelta { id, delta } => {
                if self.open_plan.insert(id.clone()) {
                    vec![
                        ModelStreamEvent::PlanStarted { id: id.clone() },
                        ModelStreamEvent::PlanDelta { id, delta },
                    ]
                } else {
                    vec![ModelStreamEvent::PlanDelta { id, delta }]
                }
            }
            ModelStreamEvent::PlanCompleted { id } => {
                self.open_plan.remove(&id);
                vec![ModelStreamEvent::PlanCompleted { id }]
            }
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
        let mut events = Vec::new();
        for key in std::mem::take(&mut self.open_text) {
            let Some((channel, id)) = key.split_once(':') else {
                continue;
            };
            events.push(ModelStreamEvent::TextCompleted {
                id: id.to_string(),
                channel: parse_channel(channel),
                authoritative_text: None,
            });
        }
        for id in std::mem::take(&mut self.open_reasoning) {
            events.push(ModelStreamEvent::ReasoningCompleted {
                id,
                provider_metadata: None,
            });
        }
        for id in std::mem::take(&mut self.open_plan) {
            events.push(ModelStreamEvent::PlanCompleted { id });
        }
        events
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

fn parse_channel(channel: &str) -> TraceTextChannel {
    match channel {
        "commentary" => TraceTextChannel::Commentary,
        "final" => TraceTextChannel::Final,
        "user" => TraceTextChannel::User,
        _ => TraceTextChannel::Final,
    }
}

fn block_key(prefix: &str, id: &str) -> String {
    format!("{prefix}:{id}")
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
