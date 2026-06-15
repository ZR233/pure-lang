use pl_trace::TraceTextChannel;

use crate::proposed_plan::{VisibleTextParser, VisibleTextSegment};

use super::event::ModelStreamEvent;

pub(crate) struct TaggedVisibleOutputAdapter {
    text_parser: VisibleTextParser,
    reasoning_parser: VisibleTextParser,
    final_id: String,
    commentary_id: String,
    plan_id: String,
}

impl TaggedVisibleOutputAdapter {
    pub(crate) fn new(plan_mode: bool) -> Self {
        Self {
            text_parser: VisibleTextParser::new(plan_mode),
            reasoning_parser: VisibleTextParser::new(plan_mode),
            final_id: "final".to_string(),
            commentary_id: "commentary".to_string(),
            plan_id: "plan".to_string(),
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
            ModelStreamEvent::TextCompleted { id, channel } => {
                if channel == TraceTextChannel::Final {
                    self.flush_visible_text()
                } else {
                    vec![ModelStreamEvent::TextCompleted { id, channel }]
                }
            }
            ModelStreamEvent::ReasoningDelta {
                id,
                chunk_index,
                delta,
            } => {
                let visible = self
                    .reasoning_parser
                    .push_str(&delta)
                    .segments
                    .into_iter()
                    .flat_map(|segment| self.segment_events(segment, false));
                std::iter::once(ModelStreamEvent::ReasoningDelta {
                    id,
                    chunk_index,
                    delta,
                })
                .chain(visible)
                .collect()
            }
            ModelStreamEvent::ReasoningCompleted {
                id,
                provider_metadata,
            } => {
                let visible = self.flush_reasoning_visible_text();
                visible
                    .into_iter()
                    .chain([ModelStreamEvent::ReasoningCompleted {
                        id,
                        provider_metadata,
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

    fn flush_visible_text(&mut self) -> Vec<ModelStreamEvent> {
        self.text_parser
            .finish()
            .segments
            .into_iter()
            .flat_map(|segment| self.segment_events(segment, true))
            .collect()
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
        self.flush_visible_text()
            .into_iter()
            .chain(self.flush_reasoning_visible_text())
            .collect()
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
            VisibleTextSegment::ProposedPlan(delta) => {
                if delta.is_empty() {
                    Vec::new()
                } else {
                    vec![ModelStreamEvent::PlanDelta {
                        id: self.plan_id.clone(),
                        delta,
                    }]
                }
            }
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
