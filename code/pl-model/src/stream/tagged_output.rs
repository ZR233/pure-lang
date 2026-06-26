use pl_trace::TraceTextChannel;

use crate::visible_text::{VisibleTextEvent, VisibleTextKind, VisibleTextParser};

use super::event::ModelStreamEvent;

pub(crate) struct TaggedVisibleOutputAdapter {
    text_parser: VisibleTextParser,
    reasoning_parser: VisibleTextParser,
    next_segment_ordinal: u64,
    active_text_block: Option<TaggedBlock>,
    active_reasoning_block: Option<TaggedBlock>,
}

#[derive(Debug, Clone)]
struct TaggedBlock {
    kind: VisibleTextKind,
    id: String,
}

impl TaggedVisibleOutputAdapter {
    pub(crate) fn new() -> Self {
        Self {
            text_parser: VisibleTextParser::new(),
            reasoning_parser: VisibleTextParser::new(),
            next_segment_ordinal: 0,
            active_text_block: None,
            active_reasoning_block: None,
        }
    }

    pub(crate) fn adapt(&mut self, event: ModelStreamEvent) -> Vec<ModelStreamEvent> {
        match event {
            ModelStreamEvent::TextStarted { id, channel } => {
                if channel == TraceTextChannel::Final {
                    Vec::new()
                } else {
                    vec![ModelStreamEvent::TextStarted { id, channel }]
                }
            }
            ModelStreamEvent::TextDelta { id, channel, delta } => {
                if channel == TraceTextChannel::Final {
                    Self::parse_visible_events(
                        &mut self.text_parser,
                        &mut self.active_text_block,
                        &mut self.next_segment_ordinal,
                        &delta,
                        true,
                    )
                } else {
                    vec![ModelStreamEvent::TextDelta { id, channel, delta }]
                }
            }
            ModelStreamEvent::TextCompleted {
                id,
                channel,
                authoritative_text,
            } => {
                if channel == TraceTextChannel::Final {
                    let mut events = Self::finish_visible_events(
                        &mut self.text_parser,
                        &mut self.active_text_block,
                        &mut self.next_segment_ordinal,
                        true,
                    );
                    if let Some(text) = authoritative_text {
                        events.extend(Self::authoritative_visible_events(
                            &mut self.next_segment_ordinal,
                            text,
                        ));
                    }
                    events
                } else {
                    vec![ModelStreamEvent::TextCompleted {
                        id,
                        channel,
                        authoritative_text,
                    }]
                }
            }
            ModelStreamEvent::ReasoningSummaryDelta { .. } => vec![event],
            ModelStreamEvent::ReasoningRawDelta {
                id,
                content_index,
                delta,
            } => {
                let visible = Self::parse_visible_events(
                    &mut self.reasoning_parser,
                    &mut self.active_reasoning_block,
                    &mut self.next_segment_ordinal,
                    &delta,
                    false,
                );
                std::iter::once(ModelStreamEvent::ReasoningRawDelta {
                    id,
                    content_index,
                    delta,
                })
                .chain(visible)
                .collect()
            }
            ModelStreamEvent::ReasoningSummaryCompleted {
                id,
                provider_metadata,
                authoritative_summary,
            } => {
                let visible = Self::finish_visible_events(
                    &mut self.reasoning_parser,
                    &mut self.active_reasoning_block,
                    &mut self.next_segment_ordinal,
                    false,
                );
                visible
                    .into_iter()
                    .chain([ModelStreamEvent::ReasoningSummaryCompleted {
                        id,
                        provider_metadata,
                        authoritative_summary,
                    }])
                    .collect()
            }
            ModelStreamEvent::Completed { response_id } => self
                .flush_all()
                .into_iter()
                .chain([ModelStreamEvent::Completed { response_id }])
                .collect(),
            ModelStreamEvent::Failed { code, message } => self
                .flush_all()
                .into_iter()
                .chain([ModelStreamEvent::Failed { code, message }])
                .collect(),
            other => vec![other],
        }
    }

    fn flush_all(&mut self) -> Vec<ModelStreamEvent> {
        Self::finish_visible_events(
            &mut self.text_parser,
            &mut self.active_text_block,
            &mut self.next_segment_ordinal,
            true,
        )
        .into_iter()
        .chain(Self::finish_visible_events(
            &mut self.reasoning_parser,
            &mut self.active_reasoning_block,
            &mut self.next_segment_ordinal,
            false,
        ))
        .collect()
    }

    fn authoritative_visible_events(
        next_segment_ordinal: &mut u64,
        text: String,
    ) -> Vec<ModelStreamEvent> {
        let mut parser = VisibleTextParser::new();
        let mut active_block = None;
        Self::parse_visible_events(
            &mut parser,
            &mut active_block,
            next_segment_ordinal,
            &text,
            true,
        )
        .into_iter()
        .chain(Self::finish_visible_events(
            &mut parser,
            &mut active_block,
            next_segment_ordinal,
            true,
        ))
        .collect()
    }

    fn parse_visible_events(
        parser: &mut VisibleTextParser,
        active_block: &mut Option<TaggedBlock>,
        next_segment_ordinal: &mut u64,
        delta: &str,
        include_untagged: bool,
    ) -> Vec<ModelStreamEvent> {
        parser
            .push_events(delta)
            .events
            .into_iter()
            .flat_map(|event| {
                Self::visible_event_events(
                    active_block,
                    next_segment_ordinal,
                    include_untagged,
                    event,
                )
            })
            .collect()
    }

    fn finish_visible_events(
        parser: &mut VisibleTextParser,
        active_block: &mut Option<TaggedBlock>,
        next_segment_ordinal: &mut u64,
        include_untagged: bool,
    ) -> Vec<ModelStreamEvent> {
        let mut events: Vec<_> = parser
            .finish_events()
            .events
            .into_iter()
            .flat_map(|event| {
                Self::visible_event_events(
                    active_block,
                    next_segment_ordinal,
                    include_untagged,
                    event,
                )
            })
            .collect();
        if let Some(block) = active_block.take() {
            events.push(ModelStreamEvent::TextCompleted {
                id: block.id,
                channel: Self::text_channel(block.kind),
                authoritative_text: None,
            });
        }
        events
    }

    fn visible_event_events(
        active_block: &mut Option<TaggedBlock>,
        next_segment_ordinal: &mut u64,
        include_untagged: bool,
        event: VisibleTextEvent,
    ) -> Vec<ModelStreamEvent> {
        match event {
            VisibleTextEvent::Untagged(text) => {
                if !include_untagged || text.is_empty() {
                    return Vec::new();
                }
                Self::text_delta(
                    active_block,
                    next_segment_ordinal,
                    VisibleTextKind::Final,
                    text,
                )
            }
            VisibleTextEvent::Open(kind) => {
                let mut events = Vec::new();
                if let Some(block) = active_block.take() {
                    events.push(ModelStreamEvent::TextCompleted {
                        id: block.id,
                        channel: Self::text_channel(block.kind),
                        authoritative_text: None,
                    });
                }
                let id = Self::next_segment_id(next_segment_ordinal, kind);
                *active_block = Some(TaggedBlock {
                    kind,
                    id: id.clone(),
                });
                events.push(ModelStreamEvent::TextStarted {
                    id,
                    channel: Self::text_channel(kind),
                });
                events
            }
            VisibleTextEvent::Delta(kind, delta) => {
                if delta.is_empty() {
                    return Vec::new();
                }
                Self::text_delta(active_block, next_segment_ordinal, kind, delta)
            }
            VisibleTextEvent::Close(_) => {
                let Some(block) = active_block.take() else {
                    return Vec::new();
                };
                vec![ModelStreamEvent::TextCompleted {
                    id: block.id,
                    channel: Self::text_channel(block.kind),
                    authoritative_text: None,
                }]
            }
        }
    }

    fn text_delta(
        active_block: &mut Option<TaggedBlock>,
        next_segment_ordinal: &mut u64,
        kind: VisibleTextKind,
        delta: String,
    ) -> Vec<ModelStreamEvent> {
        let mut events = Vec::new();
        let id = match active_block.as_ref() {
            Some(block) if block.kind == kind => block.id.clone(),
            Some(_) => {
                if let Some(block) = active_block.take() {
                    events.push(ModelStreamEvent::TextCompleted {
                        id: block.id,
                        channel: Self::text_channel(block.kind),
                        authoritative_text: None,
                    });
                }
                let id = Self::next_segment_id(next_segment_ordinal, kind);
                *active_block = Some(TaggedBlock {
                    kind,
                    id: id.clone(),
                });
                events.push(ModelStreamEvent::TextStarted {
                    id: id.clone(),
                    channel: Self::text_channel(kind),
                });
                id
            }
            None => {
                let id = Self::next_segment_id(next_segment_ordinal, kind);
                *active_block = Some(TaggedBlock {
                    kind,
                    id: id.clone(),
                });
                events.push(ModelStreamEvent::TextStarted {
                    id: id.clone(),
                    channel: Self::text_channel(kind),
                });
                id
            }
        };
        events.push(ModelStreamEvent::TextDelta {
            id,
            channel: Self::text_channel(kind),
            delta,
        });
        events
    }

    fn next_segment_id(next_segment_ordinal: &mut u64, kind: VisibleTextKind) -> String {
        *next_segment_ordinal += 1;
        format!("tagged-{}-{next_segment_ordinal}", kind.channel_label())
    }

    fn text_channel(kind: VisibleTextKind) -> TraceTextChannel {
        match kind {
            VisibleTextKind::Commentary => TraceTextChannel::Commentary,
            VisibleTextKind::Final => TraceTextChannel::Final,
        }
    }
}
