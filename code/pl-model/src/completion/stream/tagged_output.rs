use pl_trace::TraceTextChannel;

use crate::completion::visible_text::{VisibleTextEvent, VisibleTextKind, VisibleTextParser};

use super::event::{ModelBlockContent, ModelBlockKind, ModelStreamEvent};

pub(crate) struct TaggedVisibleOutputAdapter {
    text_parser: VisibleTextParser,
    reasoning_parser: VisibleTextParser,
    next_segment_ordinal: u64,
    active_text_block: Option<TaggedBlock>,
    active_reasoning_block: Option<TaggedBlock>,
    diagnostics: TaggedOutputDiagnostics,
}

#[derive(Debug, Clone)]
struct TaggedBlock {
    kind: VisibleTextKind,
    id: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct TaggedOutputDiagnostics {
    pub(crate) untagged_visible_text_segments: usize,
    pub(crate) untagged_visible_text_chars: usize,
}

impl TaggedVisibleOutputAdapter {
    pub(crate) fn new() -> Self {
        Self {
            text_parser: VisibleTextParser::new(),
            reasoning_parser: VisibleTextParser::new(),
            next_segment_ordinal: 0,
            active_text_block: None,
            active_reasoning_block: None,
            diagnostics: TaggedOutputDiagnostics::default(),
        }
    }

    pub(crate) fn diagnostics(&self) -> TaggedOutputDiagnostics {
        self.diagnostics
    }

    pub(crate) fn adapt(&mut self, event: ModelStreamEvent) -> Vec<ModelStreamEvent> {
        match event {
            ModelStreamEvent::BlockOpened {
                id,
                kind: ModelBlockKind::Text { channel },
                provider_metadata,
            } => {
                if channel == TraceTextChannel::Final {
                    Vec::new()
                } else {
                    vec![ModelStreamEvent::BlockOpened {
                        id,
                        kind: ModelBlockKind::Text { channel },
                        provider_metadata,
                    }]
                }
            }
            ModelStreamEvent::BlockDelta {
                id,
                kind: ModelBlockKind::Text { channel },
                field,
                delta,
                section_index,
            } => {
                if channel == TraceTextChannel::Final {
                    Self::parse_visible_events(
                        &mut self.text_parser,
                        &mut self.active_text_block,
                        &mut self.next_segment_ordinal,
                        &mut self.diagnostics,
                        &delta,
                        true,
                    )
                } else {
                    vec![ModelStreamEvent::BlockDelta {
                        id,
                        kind: ModelBlockKind::Text { channel },
                        field,
                        delta,
                        section_index,
                    }]
                }
            }
            ModelStreamEvent::BlockClosed {
                id,
                kind: ModelBlockKind::Text { channel },
                authoritative_content,
                provider_metadata,
            } => {
                if channel == TraceTextChannel::Final {
                    let mut events = Self::finish_visible_events(
                        &mut self.text_parser,
                        &mut self.active_text_block,
                        &mut self.next_segment_ordinal,
                        &mut self.diagnostics,
                        true,
                    );
                    if let Some(ModelBlockContent::Text(text)) = authoritative_content {
                        events.extend(Self::authoritative_visible_events(
                            &mut self.next_segment_ordinal,
                            text,
                        ));
                    }
                    events
                } else {
                    vec![ModelStreamEvent::BlockClosed {
                        id,
                        kind: ModelBlockKind::Text { channel },
                        authoritative_content,
                        provider_metadata,
                    }]
                }
            }
            ModelStreamEvent::BlockOpened { .. } | ModelStreamEvent::BlockDelta { .. } => {
                vec![event]
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
            ModelStreamEvent::BlockClosed {
                id,
                kind: ModelBlockKind::ReasoningSummary,
                authoritative_content,
                provider_metadata,
            } => {
                let visible = Self::finish_visible_events(
                    &mut self.reasoning_parser,
                    &mut self.active_reasoning_block,
                    &mut self.next_segment_ordinal,
                    &mut self.diagnostics,
                    false,
                );
                visible
                    .into_iter()
                    .chain([ModelStreamEvent::BlockClosed {
                        id,
                        kind: ModelBlockKind::ReasoningSummary,
                        authoritative_content,
                        provider_metadata,
                    }])
                    .collect()
            }
            ModelStreamEvent::Completed { response_id } => self
                .flush_all()
                .into_iter()
                .chain([ModelStreamEvent::Completed { response_id }])
                .collect(),
            ModelStreamEvent::Failed {
                code,
                http_status,
                retry_after_ms,
                message,
            } => self
                .flush_all()
                .into_iter()
                .chain([ModelStreamEvent::Failed {
                    code,
                    http_status,
                    retry_after_ms,
                    message,
                }])
                .collect(),
            event @ (ModelStreamEvent::ToolInputStarted { .. }
            | ModelStreamEvent::ToolInputDelta { .. }
            | ModelStreamEvent::ToolCallReady { .. }
            | ModelStreamEvent::ResponseStarted { .. }) => self
                .flush_visible_text()
                .into_iter()
                .chain([event])
                .collect(),
            other => vec![other],
        }
    }

    fn flush_visible_text(&mut self) -> Vec<ModelStreamEvent> {
        Self::finish_visible_events(
            &mut self.text_parser,
            &mut self.active_text_block,
            &mut self.next_segment_ordinal,
            &mut self.diagnostics,
            true,
        )
    }

    fn flush_all(&mut self) -> Vec<ModelStreamEvent> {
        self.flush_visible_text()
            .into_iter()
            .chain(Self::finish_visible_events(
                &mut self.reasoning_parser,
                &mut self.active_reasoning_block,
                &mut self.next_segment_ordinal,
                &mut self.diagnostics,
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
        let mut diagnostics = TaggedOutputDiagnostics::default();
        Self::parse_visible_events(
            &mut parser,
            &mut active_block,
            next_segment_ordinal,
            &mut diagnostics,
            &text,
            true,
        )
        .into_iter()
        .chain(Self::finish_visible_events(
            &mut parser,
            &mut active_block,
            next_segment_ordinal,
            &mut diagnostics,
            true,
        ))
        .collect()
    }

    fn parse_visible_events(
        parser: &mut VisibleTextParser,
        active_block: &mut Option<TaggedBlock>,
        next_segment_ordinal: &mut u64,
        diagnostics: &mut TaggedOutputDiagnostics,
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
                    diagnostics,
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
        diagnostics: &mut TaggedOutputDiagnostics,
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
                    diagnostics,
                    include_untagged,
                    event,
                )
            })
            .collect();
        if let Some(block) = active_block.take() {
            events.push(ModelStreamEvent::text_completed(
                block.id,
                Self::text_channel(block.kind),
                None,
            ));
        }
        events
    }

    fn visible_event_events(
        active_block: &mut Option<TaggedBlock>,
        next_segment_ordinal: &mut u64,
        diagnostics: &mut TaggedOutputDiagnostics,
        include_untagged: bool,
        event: VisibleTextEvent,
    ) -> Vec<ModelStreamEvent> {
        match event {
            VisibleTextEvent::Untagged(text) => {
                if !include_untagged || text.is_empty() {
                    return Vec::new();
                }
                diagnostics.untagged_visible_text_segments += 1;
                diagnostics.untagged_visible_text_chars += text.len();
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
                    events.push(ModelStreamEvent::text_completed(
                        block.id,
                        Self::text_channel(block.kind),
                        None,
                    ));
                }
                let id = Self::next_segment_id(next_segment_ordinal, kind);
                *active_block = Some(TaggedBlock {
                    kind,
                    id: id.clone(),
                });
                events.push(ModelStreamEvent::text_started(id, Self::text_channel(kind)));
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
                vec![ModelStreamEvent::text_completed(
                    block.id,
                    Self::text_channel(block.kind),
                    None,
                )]
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
                    events.push(ModelStreamEvent::text_completed(
                        block.id,
                        Self::text_channel(block.kind),
                        None,
                    ));
                }
                let id = Self::next_segment_id(next_segment_ordinal, kind);
                *active_block = Some(TaggedBlock {
                    kind,
                    id: id.clone(),
                });
                events.push(ModelStreamEvent::text_started(
                    id.clone(),
                    Self::text_channel(kind),
                ));
                id
            }
            None => {
                let id = Self::next_segment_id(next_segment_ordinal, kind);
                *active_block = Some(TaggedBlock {
                    kind,
                    id: id.clone(),
                });
                events.push(ModelStreamEvent::text_started(
                    id.clone(),
                    Self::text_channel(kind),
                ));
                id
            }
        };
        events.push(ModelStreamEvent::text_delta(
            id,
            Self::text_channel(kind),
            delta,
        ));
        events
    }

    fn next_segment_id(next_segment_ordinal: &mut u64, kind: VisibleTextKind) -> String {
        *next_segment_ordinal += 1;
        let label = kind.channel_label();
        format!("tagged-{label}-{next_segment_ordinal}")
    }

    fn text_channel(kind: VisibleTextKind) -> TraceTextChannel {
        match kind {
            VisibleTextKind::Commentary => TraceTextChannel::Commentary,
            VisibleTextKind::Final => TraceTextChannel::Final,
        }
    }
}
