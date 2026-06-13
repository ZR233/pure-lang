use std::collections::{HashMap, HashSet};

use pl_protocol::TimelineTextChannel;

use super::event::{ModelStreamEvent, ToolInputDeltaPayload, ToolInputPayloadKind};

pub(crate) struct StreamLifecycle {
    open_text: HashSet<String>,
    open_reasoning: HashSet<String>,
    open_plan: HashSet<String>,
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
            open_text: HashSet::new(),
            open_reasoning: HashSet::new(),
            open_plan: HashSet::new(),
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
            ModelStreamEvent::TextCompleted { id, channel } => {
                self.open_text.remove(&block_key(channel.as_str(), &id));
                vec![ModelStreamEvent::TextCompleted { id, channel }]
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
                self.upsert_tool_input(
                    stream_id.as_ref(),
                    &item_id,
                    call_id.as_ref(),
                    name.as_ref(),
                );
                vec![ModelStreamEvent::ToolInputStarted {
                    stream_id,
                    item_id,
                    call_id,
                    name,
                    payload_kind,
                }]
            }
            ModelStreamEvent::ToolInputDelta {
                stream_id,
                item_id,
                call_id,
                name,
                payload_delta,
            } => {
                let key = tool_key(stream_id.as_ref(), call_id.as_ref(), &item_id);
                let is_new = !self.open_tools.contains_key(&key);
                self.upsert_tool_input(
                    stream_id.as_ref(),
                    &item_id,
                    call_id.as_ref(),
                    name.as_ref(),
                );
                if is_new {
                    vec![
                        ModelStreamEvent::ToolInputStarted {
                            stream_id: stream_id.clone(),
                            item_id: item_id.clone(),
                            call_id: call_id.clone(),
                            name: name.clone(),
                            payload_kind: payload_delta.kind(),
                        },
                        ModelStreamEvent::ToolInputDelta {
                            stream_id,
                            item_id,
                            call_id,
                            name,
                            payload_delta,
                        },
                    ]
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
                self.open_tools
                    .remove(&tool_key(stream_id.as_ref(), call_id.as_ref(), &item_id));
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
                vec![ModelStreamEvent::StepStarted { response_id }]
            }
            ModelStreamEvent::ToolCallReady {
                stream_id,
                item_id,
                call_id,
                name,
                payload,
            } => {
                self.open_tools
                    .remove(&tool_key(stream_id.as_ref(), call_id.as_ref(), &item_id));
                vec![ModelStreamEvent::ToolCallReady {
                    stream_id,
                    item_id,
                    call_id,
                    name,
                    payload,
                }]
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
        let mut events = Vec::new();
        for key in std::mem::take(&mut self.open_text) {
            let Some((channel, id)) = key.split_once(':') else {
                continue;
            };
            events.push(ModelStreamEvent::TextCompleted {
                id: id.to_string(),
                channel: parse_channel(channel),
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

    fn upsert_tool_input(
        &mut self,
        stream_id: Option<&String>,
        item_id: &str,
        call_id: Option<&String>,
        name: Option<&String>,
    ) {
        let key = tool_key(stream_id, call_id, item_id);
        let entry = self.open_tools.entry(key).or_insert_with(|| OpenToolInput {
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
}

fn parse_channel(channel: &str) -> TimelineTextChannel {
    match channel {
        "commentary" => TimelineTextChannel::Commentary,
        "final" => TimelineTextChannel::Final,
        "user" => TimelineTextChannel::User,
        _ => TimelineTextChannel::Final,
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
