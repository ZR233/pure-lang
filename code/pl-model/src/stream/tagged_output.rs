use pl_trace::TraceTextChannel;

use crate::visible_text::{VisibleTextParser, VisibleTextSegment};

use super::event::ModelStreamEvent;

pub(crate) struct TaggedVisibleOutputAdapter {
    text_parser: VisibleTextParser,
    reasoning_parser: VisibleTextParser,
    final_id: String,
    commentary_id: String,
}

impl TaggedVisibleOutputAdapter {
    pub(crate) fn new() -> Self {
        Self {
            text_parser: VisibleTextParser::new(),
            reasoning_parser: VisibleTextParser::new(),
            final_id: "final".to_string(),
            commentary_id: "commentary".to_string(),
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
                    self.visible_text_delta(delta)
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
                    self.flush_visible_text(authoritative_text)
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
                let visible = self
                    .reasoning_parser
                    .push_str(&delta)
                    .segments
                    .into_iter()
                    .flat_map(|segment| self.segment_events(segment, false));
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
                let visible = self.flush_reasoning_visible_text();
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

    fn visible_text_delta(&mut self, delta: String) -> Vec<ModelStreamEvent> {
        self.text_parser
            .push_str(&delta)
            .segments
            .into_iter()
            .flat_map(|segment| self.segment_events(segment, true))
            .collect()
    }

    fn flush_visible_text(&mut self, authoritative_text: Option<String>) -> Vec<ModelStreamEvent> {
        let mut events: Vec<_> = self
            .text_parser
            .finish()
            .segments
            .into_iter()
            .flat_map(|segment| self.segment_events(segment, true))
            .collect();
        if let Some(text) = authoritative_text {
            events.extend(self.authoritative_segment_completed_events(text));
        }
        events
    }

    fn flush_reasoning_visible_text(&mut self) -> Vec<ModelStreamEvent> {
        self.reasoning_parser
            .finish()
            .segments
            .into_iter()
            .flat_map(|segment| self.segment_events(segment, false))
            .collect()
    }

    fn flush_all(&mut self) -> Vec<ModelStreamEvent> {
        self.flush_visible_text(None)
            .into_iter()
            .chain(self.flush_reasoning_visible_text())
            .collect()
    }

    fn authoritative_segment_completed_events(&self, text: String) -> Vec<ModelStreamEvent> {
        let mut parser = VisibleTextParser::new();
        let mut segments = parser.push_str(&text).segments;
        segments.extend(parser.finish().segments);
        let mut events = Vec::new();
        for segment in segments {
            match segment {
                VisibleTextSegment::Untagged(text) | VisibleTextSegment::Final(text) => {
                    if !text.trim().is_empty() {
                        events.push(ModelStreamEvent::TextCompleted {
                            id: self.final_id.clone(),
                            channel: TraceTextChannel::Final,
                            authoritative_text: Some(text),
                        });
                    }
                }
                VisibleTextSegment::Commentary(text) => {
                    if !text.trim().is_empty() {
                        events.push(ModelStreamEvent::TextCompleted {
                            id: self.commentary_id.clone(),
                            channel: TraceTextChannel::Commentary,
                            authoritative_text: Some(text),
                        });
                    }
                }
            }
        }
        events
    }

    fn segment_events(
        &self,
        segment: VisibleTextSegment,
        include_untagged: bool,
    ) -> Vec<ModelStreamEvent> {
        match segment {
            VisibleTextSegment::Untagged(text) => {
                if !include_untagged || text.trim().is_empty() {
                    Vec::new()
                } else {
                    self.text_delta(self.final_id.clone(), TraceTextChannel::Final, text)
                }
            }
            VisibleTextSegment::Final(text) => {
                self.text_delta(self.final_id.clone(), TraceTextChannel::Final, text)
            }
            VisibleTextSegment::Commentary(text) => self.text_delta(
                self.commentary_id.clone(),
                TraceTextChannel::Commentary,
                text,
            ),
        }
    }

    fn text_delta(
        &self,
        id: String,
        channel: TraceTextChannel,
        delta: String,
    ) -> Vec<ModelStreamEvent> {
        if delta.is_empty() {
            Vec::new()
        } else {
            vec![ModelStreamEvent::TextDelta { id, channel, delta }]
        }
    }
}
